mod db;
mod models;
mod roadmap;

fn main() {
    let _ = db::Db::open();
}
