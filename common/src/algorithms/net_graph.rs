use std::{
    collections::HashMap,
    fmt::{self, Display},
    iter::FromIterator,
    net::IpAddr,
};

use petgraph::{algo::astar::astar, graph::NodeIndex, Graph};

use crate::domain::{Hostname, MachineStatus};

#[derive(Debug, Default, PartialEq, Eq, Clone)]
struct Routing {
    table: HashMap<NodeIndex<u32>, IpAddr>,
}

impl Routing {
    fn connect_to(&mut self, node: NodeIndex<u32>, machine: &MachineStatus) {
        self.table.insert(node, machine.external_ip);
    }

    fn get(&self, node: &NodeIndex<u32>) -> Option<IpAddr> {
        self.table.get(node).copied()
    }
}

#[derive(Debug, PartialEq, Eq, Clone)]
enum Node<'hostname> {
    Machine(&'hostname MachineStatus),
    Internet(Routing),
}

impl Display for Node<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Node::Machine(m) => write!(
                f,
                "M({}@{:?})",
                m.hostname.as_ref(),
                m.ip_connections[0].local_ip
            ),
            Node::Internet(_) => write!(f, "[I]"),
        }
    }
}

impl<'hostname> Node<'hostname> {
    fn is_host(&self, host: &Hostname) -> bool {
        matches!(self, Node::Machine(MachineStatus { hostname, .. }) if hostname == host)
    }

    fn share_nat(&self, other: &MachineStatus) -> bool {
        matches!(
            self,
            Node::Machine(MachineStatus { external_ip, .. }) if *external_ip == other.external_ip
        )
    }

    fn unwrap_as_internet_mut(&mut self) -> &mut Routing {
        match self {
            Self::Internet(r) => r,
            _ => panic!("Unwrapped as internet but self is {:?}", self),
        }
    }
}

pub struct NetGraph<'hostname> {
    graph: Graph<Node<'hostname>, usize>,
}

impl<'hostname> NetGraph<'hostname> {
    const INTERNET_WEIGHT: usize = 100;
    const INTRANET_WEIGHT: usize = 1;
}

impl<'hostname> FromIterator<&'hostname MachineStatus> for NetGraph<'hostname> {
    fn from_iter<T: IntoIterator<Item = &'hostname MachineStatus>>(iter: T) -> Self {
        let mut graph = Graph::new();

        // create the internet
        let internet_idx = graph.add_node(Node::Internet(Default::default()));

        for machine in iter {
            let machine_idx = graph.add_node(Node::Machine(machine)); // create a machine

            // connect machine to internet
            graph.add_edge(machine_idx, internet_idx, Self::INTERNET_WEIGHT);

            // establish a port forwward
            if machine.ssh.is_some() {
                graph.add_edge(internet_idx, machine_idx, Self::INTERNET_WEIGHT);
                graph[internet_idx]
                    .unwrap_as_internet_mut()
                    .connect_to(machine_idx, machine);
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

impl<'hostname> NetGraph<'hostname> {
    pub fn find_path(&self, from: &Hostname, to: &Hostname) -> Option<Vec<IpAddr>> {
        let graph = &self.graph;
        let from = graph.node_indices().find(|i| graph[*i].is_host(from))?;

        let (_, nodes) = astar(
            graph,
            from,
            |i| graph[i].is_host(to),
            |e| *e.weight(),
            |_| 0,
        )?;
        let mut i = nodes.into_iter();
        i.next(); // skip myself
        let mut v = vec![];
        while let Some(ni) = i.next() {
            match &graph[ni] {
                Node::Machine(n) => v.push(n.ip_connections.first()?.local_ip),
                Node::Internet(routing) => {
                    // the next one will have the ip determined by the routing table
                    let ni = i.next().expect("a path can't end on the internet");
                    let ip = routing
                        .get(&ni)
                        .expect("the internet must now all paths it leads to");
                    v.push(ip);
                }
            }
        }
        Some(v)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::domain::{
        hostname::tests::FakeHostname,
        machine_status::{IpConnection, MachineStatus},
    };
    use fake::{faker::internet::en::IP, Fake};

    fn mock_machine_status() -> MachineStatus {
        MachineStatus {
            hostname: FakeHostname.fake(),
            ip_connections: vec![IpConnection {
                local_ip: IP().fake(),
                gateway_ip: IP().fake(),
                gateway_mac: None,
            }],
            external_ip: IP().fake(),
            ssh: None,
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
        let path = NetGraph::from_iter(&v).find_path(&v[0].hostname, &v[1].hostname);
        assert_eq!(path, Some(vec![v[1].ip_connections[0].local_ip]))
    }

    #[test]
    fn internet_one_hop() {
        let host1 = mock_machine_status();
        let host2 = mock_machine_status().also(|m| m.ssh = Some(22));
        let v = [host1, host2];
        let path = NetGraph::from_iter(&v).find_path(&v[0].hostname, &v[1].hostname);
        assert_eq!(path, Some(vec![v[1].external_ip]))
    }

    #[test]
    fn internet_two_hops() {
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
        let path = NetGraph::from_iter(&v).find_path(&v[0].hostname, &v[2].hostname);
        assert_eq!(
            path,
            Some(vec![v[1].external_ip, v[2].ip_connections[0].local_ip])
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
