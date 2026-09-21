mod analytics;
mod db;
mod github;
mod mentor;
mod models;
mod quiz;
mod roadmap;
mod srs;
mod verify;

fn main() {
    let _ = db::Db::open();
}
