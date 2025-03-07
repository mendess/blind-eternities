use std::{
    collections::{HashMap, hash_map::Entry},
    fmt::{self, Display},
    io,
    iter::FromIterator,
    net::{IpAddr, Ipv6Addr},
};

use chrono::{Duration, Utc};
use petgraph::{Graph, algo::astar::astar, graph::NodeIndex};
use tokio::io::{AsyncWrite, AsyncWriteExt};

use crate::domain::{
    Hostname, MachineStatus,
    machine_status::{MachineStatusFull, Port},
};

#[derive(Debug, Default, PartialEq, Eq, Clone)]
struct Internet {
    table: HashMap<NodeIndex<u32>, (IpAddr, Port)>,
}

impl Internet {
    fn connect_to(&mut self, node: NodeIndex<u32>, machine: IpAddr, port: Port) {
        self.table.insert(node, (machine, port));
    }

    fn get(&self, node: &NodeIndex<u32>) -> Option<(IpAddr, Port)> {
        self.table.get(node).copied()
    }
}

#[derive(Debug, PartialEq, Eq, Clone)]
enum Node<'hostname> {
    Machine(&'hostname MachineStatusFull),
    Internet(Internet),
}

impl Display for Node<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Node::Machine(m) => match m.preferred_ip() {
                Some(ip) => write!(f, "M({}@{ip})", m.hostname.as_ref()),
                None => write!(f, "M({})", m.hostname.as_ref()),
            },
            Node::Internet(_) => write!(f, "[I]"),
        }
    }
}

impl Node<'_> {
    fn is_host(&self, host: &Hostname) -> bool {
        matches!(self, Node::Machine(m) if m.hostname == *host)
    }

    fn share_nat(&self, other: &MachineStatus) -> bool {
        fn ip_eq(a: IpAddr, b: IpAddr) -> bool {
            match (a, b) {
                (IpAddr::V4(a), IpAddr::V4(b)) => a == b,
                (IpAddr::V6(a), IpAddr::V6(b)) => a.octets()[0..8] == b.octets()[0..8],
                _ => false,
            }
        }
        matches!(self, Node::Machine(m) if ip_eq(m.external_ip, other.external_ip))
    }

    fn unwrap_as_internet_mut(&mut self) -> &mut Internet {
        match self {
            Self::Internet(r) => r,
            _ => panic!("Unwrapped as internet but self is {:?}", self),
        }
    }

    fn unwrap_as_machine(&self) -> &MachineStatusFull {
        match self {
            Self::Machine(r) => r,
            _ => panic!("Unwrapped as internet but self is {:?}", self),
        }
    }
}

#[derive(Debug)]
pub struct NetGraph<'hostname> {
    graph: Graph<Node<'hostname>, usize>,
}

impl NetGraph<'_> {
    const INTERNET_WEIGHT: usize = 100;
    const INTRANET_WEIGHT: usize = 1;
}

impl<'hostname> FromIterator<&'hostname MachineStatusFull> for NetGraph<'hostname> {
    fn from_iter<T: IntoIterator<Item = &'hostname MachineStatusFull>>(iter: T) -> Self {
        let mut graph = Graph::new();

        // create the internet
        let internet_idx = graph.add_node(Node::Internet(Default::default()));

        for machine in iter {
            // create a machine
            let machine_idx = graph.add_node(Node::Machine(machine));

            // connect machine to internet
            graph.add_edge(machine_idx, internet_idx, Self::INTERNET_WEIGHT);

            // establish a port forwward
            if let Some(port) = machine.ssh {
                graph.add_edge(internet_idx, machine_idx, Self::INTERNET_WEIGHT);
                graph[internet_idx].unwrap_as_internet_mut().connect_to(
                    machine_idx,
                    machine.external_ip,
                    port,
                );
            }

            // find all the friends of this network
            let subnet_friends = graph
                .node_indices()
                .filter(|i| graph[*i].share_nat(machine))
                .collect::<Vec<_>>();

            // connect both ways with friends
            for friend in subnet_friends {
                graph.add_edge(machine_idx, friend, Self::INTRANET_WEIGHT);
                graph.add_edge(friend, machine_idx, Self::INTRANET_WEIGHT);
            }
        }
        Self { graph }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SimpleNode {
    pub default_username: Option<String>,
    pub ip: IpAddr,
    pub port: Port,
}

impl NetGraph<'_> {
    pub fn find_path(&self, from: &Hostname, to: &Hostname) -> Option<Vec<NodeIndex<u32>>> {
        let graph = &self.graph;
        let from = graph.node_indices().find(|i| graph[*i].is_host(from))?;

        let (_, nodes) = astar(
            graph,
            from,
            |i| graph[i].is_host(to),
            |e| *e.weight(),
            |_| 0,
        )?;
        Some(nodes)
    }

    pub fn path_to_ips(&self, nodes: &[NodeIndex<u32>]) -> Option<Vec<SimpleNode>> {
        let mut i = nodes.iter();
        let mut v = vec![];
        while let Some(ni) = i.next() {
            match &self.graph[*ni] {
                Node::Machine(n) => {
                    let ip = n.preferred_ip()?;
                    v.push(SimpleNode {
                        default_username: n.default_user.clone(),
                        ip,
                        port: 22,
                    })
                }
                Node::Internet(routing) => {
                    // the next one will have the ip determined by the routing table
                    let ni = i.next().expect("a path can't end on the internet");
                    let (ip, port) = routing
                        .get(ni)
                        .expect("the internet must know all paths it leads to");
                    v.push(SimpleNode {
                        default_username: self.graph[*ni].unwrap_as_machine().default_user.clone(),
                        ip,
                        port,
                    });
                }
            }
        }
        Some(v)
    }

    pub async fn to_dot<W: AsyncWrite>(
        &self,
        out: W,
        path: Option<&[NodeIndex<u32>]>,
    ) -> io::Result<()> {
        fn external_ip_to_subnet(ip: IpAddr) -> String {
            match ip {
                IpAddr::V4(ip) => ip.to_string(),
                IpAddr::V6(ip) => {
                    let mut octets = ip.octets();
                    octets[8..].fill(0);
                    Ipv6Addr::from(octets).to_string()
                }
            }
        }
        const COLOR_NAME: &str = r#" color="cornflowerblue""#;
        tokio::pin!(out);

        out.write_all(b"digraph {\n    node [colorscheme=rdylgn9]\n")
            .await?;
        let mut by_subnet = HashMap::<_, Vec<_>>::new();
        let mut internet = None;
        for i in self.graph.node_indices() {
            if let Node::Machine(s) = self.graph[i] {
                by_subnet
                    .entry(external_ip_to_subnet(s.external_ip))
                    .or_default()
                    .push((i, s));
            } else {
                internet = Some(i);
            }
        }
        let internet = internet.unwrap();
        out.write_all(
            format!(
                "    {} [ label = \"{}\" ]\n",
                internet.index(),
                self.graph[internet]
            )
            .as_bytes(),
        )
        .await?;

        let (today, one_hour_ago) = {
            let today = Utc::now();
            let one_hour_ago = today - Duration::try_hours(1).unwrap();

            (today.timestamp_millis(), one_hour_ago.timestamp_millis())
        };
        for (ip, nodes) in by_subnet.into_iter() {
            let subgraph_label = ip.to_string().replace(['.', ':'], "_");
            out.write_all(format!("    subgraph cluster_{} {{\n", subgraph_label).as_bytes())
                .await?;
            for (i, n) in nodes {
                let hb = n.last_heartbeat.timestamp_millis();
                let color = if hb < one_hour_ago {
                    tracing::info!("node: {} @ {:?} :: {}", n.hostname, n.last_heartbeat, 1);
                    tracing::debug!("node: {:#?} :: {}", n, 1);
                    String::from(" style=filled fillcolor=1")
                } else {
                    let color = 1 + ((7 * (hb - one_hour_ago)) / (today - one_hour_ago));
                    tracing::info!("node: {} @ {:?} :: {}", n.hostname, n.last_heartbeat, color);
                    tracing::debug!("node: {:#?} :: {}", n, color);
                    format!(" style=filled fillcolor={}", color)
                };
                out.write_all(
                    format!(
                        "        {} [ label = \"{}{}\" {color} ]\n",
                        i.index(),
                        Node::Machine(n),
                        if hb < one_hour_ago {
                            format!("\n{}", n.last_heartbeat)
                        } else {
                            String::new()
                        },
                    )
                    .as_bytes(),
                )
                .await?;
            }
            out.write_all(format!("        label = \"{}\"\n", ip).as_bytes())
                .await?;
            out.write_all(b"    }\n").await?;
        }

        let mut edges = HashMap::new();
        for e in self.graph.raw_edges() {
            if e.source() == e.target() {
                continue;
            }
            let mut a = [e.source(), e.target()];
            a.sort();
            match edges.entry(a) {
                Entry::Vacant(v) => {
                    v.insert(([e.source(), e.target()], e.weight, false));
                }
                Entry::Occupied(mut o) => {
                    o.insert(([e.source(), e.target()], e.weight, true));
                }
            }
        }
        for (_, (edge @ [from, to], w, bidirectional)) in edges {
            let s = format!(
                "    {} -> {} [ label = \"{}\"{}{} ]\n",
                from.index(),
                to.index(),
                w,
                if bidirectional { r#" dir="both""# } else { "" },
                if let Some(true) = path.map(|nodes| {
                    nodes
                        .windows(2)
                        .any(|n| n == edge || (bidirectional && n == [to, from]))
                }) {
                    COLOR_NAME
                } else {
                    ""
                }
            );
            out.write_all(s.as_bytes()).await?;
        }
        out.write_all(b"}\n").await?;
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::domain::{
        hostname::tests::FakeHostname,
        machine_status::{IpConnection, MachineStatus},
    };
    use chrono::Utc;
    use fake::{Fake, faker::internet::en::IP};

    fn mock_machine_status() -> MachineStatusFull {
        MachineStatusFull {
            fields: MachineStatus {
                hostname: FakeHostname.fake(),
                ip_connections: vec![IpConnection {
                    local_ip: IP().fake(),
                    gateway_ip: IP().fake(),
                    gateway_mac: None,
                }],
                external_ip: IP().fake(),
                ssh: None,
                default_user: None,
            },
            last_heartbeat: Utc::now(),
        }
    }

    trait Also {
        fn also<F: FnOnce(&mut Self)>(self, f: F) -> Self;
    }

    impl<T> Also for T {
        fn also<F: FnOnce(&mut Self)>(mut self, f: F) -> Self {
            f(&mut self);
            self
        }
    }

    #[test]
    fn empty() {
        let v = vec![];
        assert_eq!(
            NetGraph::from_iter(&v).find_path(&FakeHostname.fake(), &FakeHostname.fake()),
            None
        );
    }

    #[test]
    fn lan() {
        let external_ip = IP().fake();
        let host1 = mock_machine_status().also(|m| m.external_ip = external_ip);
        let host2 = mock_machine_status().also(|m| m.external_ip = external_ip);
        let v = [host1, host2];
        let netgraph = NetGraph::from_iter(&v);
        let path =
            netgraph.path_to_ips(&netgraph.find_path(&v[0].hostname, &v[1].hostname).unwrap());
        assert_eq!(
            path,
            Some(vec![
                SimpleNode {
                    default_username: None,
                    ip: v[0].ip_connections[0].local_ip,
                    port: 22
                },
                SimpleNode {
                    default_username: None,
                    ip: v[1].ip_connections[0].local_ip,
                    port: 22
                }
            ])
        )
    }

    #[test]
    fn internet_one_hop() {
        let host1 = mock_machine_status();
        let host2 = mock_machine_status().also(|m| m.ssh = Some(222));
        let v = [host1, host2];
        let netgraph = NetGraph::from_iter(&v);
        let path =
            netgraph.path_to_ips(&netgraph.find_path(&v[0].hostname, &v[1].hostname).unwrap());
        assert_eq!(
            path,
            Some(vec![
                SimpleNode {
                    default_username: None,
                    ip: v[0].ip_connections[0].local_ip,
                    port: 22,
                },
                SimpleNode {
                    default_username: None,
                    ip: v[1].external_ip,
                    port: 222,
                }
            ])
        )
    }

    #[test]
    fn internet_two_hops() {
        let host1 = mock_machine_status();
        let (host2, host3) = {
            let external_ip = IP().fake();
            let host2 = mock_machine_status()
                .also(|m| m.external_ip = external_ip)
                .also(|m| m.ssh = Some(222));
            let host3 = mock_machine_status().also(|m| m.external_ip = external_ip);
            (host2, host3)
        };
        let v = [host1, host2, host3];
        let netgraph = NetGraph::from_iter(&v);
        let path =
            netgraph.path_to_ips(&netgraph.find_path(&v[0].hostname, &v[2].hostname).unwrap());
        assert_eq!(
            path,
            Some(vec![
                SimpleNode {
                    default_username: None,
                    ip: v[0].ip_connections[0].local_ip,
                    port: 22,
                },
                SimpleNode {
                    default_username: None,
                    ip: v[1].external_ip,
                    port: 222,
                },
                SimpleNode {
                    default_username: None,
                    ip: v[2].ip_connections[0].local_ip,
                    port: 22,
                }
            ])
        )
    }

    #[test]
    fn impossible_hop() {
        let host1 = mock_machine_status();
        let (host2, host3) = {
            let external_ip = IP().fake();
            let host2 = mock_machine_status()
                .also(|m| m.external_ip = external_ip)
                .also(|m| m.ssh = Some(22));
            let host3 = mock_machine_status().also(|m| m.external_ip = external_ip);
            (host2, host3)
        };
        let v = [host1, host2, host3];
        let path = NetGraph::from_iter(&v).find_path(&v[2].hostname, &v[0].hostname);
        assert_eq!(path, None)
    }
}
