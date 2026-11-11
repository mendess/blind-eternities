use askama::Template;
use axum::{
    Router,
    response::{Html, IntoResponse},
    routing::get,
};

use scryfall::card::{Card, Color};

pub fn router() -> Router<RouterState> {
    Router::new()
        .route("/", get(index))
        .route("/pair/{pair}", get(pair))
}

#[derive(Template)]
#[template(path = "games/mtg/good-lands.html")]
struct GoodLands {
    pairs: Vec<(&'static str, [Color; 2])>,
}

pub async fn index() -> Result<impl IntoResponse, super::Error> {
    Ok(Html(
        GoodLands {
            pairs: PAIRS.to_vec(),
        }
        .render()?,
    ))
}

#[derive(Template)]
#[template(path = "games/mtg/good-lands.frag.list.html")]
struct ListFragment {
    cards: Vec<String>,
}

pub async fn pair(pair: axum::extract::Path<String>) -> Result<impl IntoResponse, super::Error> {
    Ok(Html(
        ListFragment {
            cards: generate_list(
                PAIRS
                    .iter()
                    .find(|p| p.0 == pair.0)
                    .ok_or_else(|| super::Error::BadRequest)?
                    .1,
            )
            .await
            .map_err(|e| super::Error::InternalServerError(e.to_string()))?,
        }
        .render()?,
    ))
}

use crate::RouterState;
pub use pair::Pair;
pub mod pair {
    pub type Pair = [Color; 2];
    use scryfall::card::Color;

    pub const WU: Pair = [Color::White, Color::Blue];
    pub const UB: Pair = [Color::Blue, Color::Black];
    pub const BR: Pair = [Color::Black, Color::Red];
    pub const RG: Pair = [Color::Red, Color::Green];
    pub const GW: Pair = [Color::Green, Color::White];

    pub const WB: Pair = [Color::White, Color::Black];
    pub const UR: Pair = [Color::Blue, Color::Red];
    pub const BG: Pair = [Color::Black, Color::Green];
    pub const RW: Pair = [Color::Red, Color::White];
    pub const GU: Pair = [Color::Green, Color::Blue];
}

const PAIRS: [(&str, [Color; 2]); 10] = [
    ("wu", pair::WU),
    ("ub", pair::UB),
    ("br", pair::BR),
    ("rg", pair::RG),
    ("gw", pair::GW),
    ("wb", pair::WB),
    ("ur", pair::UR),
    ("bg", pair::BG),
    ("rw", pair::RW),
    ("gu", pair::GU),
];

// The ten Onslaught/Zendikar fetchlands and five Mirage slow fetches.
const FETCHLANDS: [(Pair, &str); 15] = [
    (pair::RW, "Arid Mesa"),
    (pair::BR, "Bloodstained Mire"),
    (pair::WU, "Flooded Strand"),
    (pair::WB, "Marsh Flats"),
    (pair::GU, "Misty Rainforest"),
    (pair::UB, "Polluted Delta"),
    (pair::UR, "Scalding Tarn"),
    (pair::BG, "Verdant Catacombs"),
    (pair::GW, "Windswept Heath"),
    (pair::RG, "Wooded Foothills"),
    (pair::UB, "Bad River"),
    (pair::WU, "Flood Plain"),
    (pair::GW, "Grasslands"),
    (pair::RG, "Mountain Valley"),
    (pair::BR, "Rocky Tar Pit"),
];

const EXCLUDED_ORACLE_TAGS: &[&str] = &[
    "cycle-guildgate",
    "cycle-ice-depletion-land",
    "cycle-inv-dual-tapland",
    "cycle-ktk-gainland",
    "cycle-ogw-gainland",
    "cycle-ogw-tapland",
    "cycle-signet-land",
    "cycle-snc-c-tapland",
    "cycle-soi-tapland",
    "cycle-sos-surveil-dual",
    "cycle-stx-campus",
    "cycle-napland",
    "cycle-tmp-pain-tapland",
    "cycle-zen-refugeland",
    "storage-land",
];

fn land_query(pair: Pair) -> String {
    let produces = format!(
        "(produces={lhs}{rhs} or produces=C{lhs}{rhs})",
        lhs = pair[0],
        rhs = pair[1]
    );
    let mut query = String::from("t:land game:paper -is:funny -is:ub -is:mdfc");
    query.push(' ');
    query.push_str(&produces);
    for tag in EXCLUDED_ORACLE_TAGS {
        query.push_str(" -otag:");
        query.push_str(tag);
    }
    query
}

pub async fn generate_list(pair: Pair) -> Result<Vec<String>, scryfall::Error> {
    let mut names = Vec::new();
    let cards = Card::search_all(land_query(pair)).await?;
    for card in cards {
        names.push(card.name);
    }
    names.extend(
        FETCHLANDS
            .iter()
            .filter(|(p, _)| p == &pair)
            .map(|(_, n)| n.to_string()),
    );
    names.sort();
    names.dedup();

    Ok(names)
}
