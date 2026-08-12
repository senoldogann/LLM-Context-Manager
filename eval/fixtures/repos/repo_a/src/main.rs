mod audit;
mod config;
mod db;
mod inventory;
mod notify;
mod order;
mod pricing;
mod report;
mod shipping;
mod tax;
mod utils;
mod validation;

use std::process::ExitCode;

fn main() -> ExitCode {
    let conn = db::connect("warehouse.db");
    let orders = order::load_open_orders(&conn);
    for order in orders {
        let total = pricing::compute_total(&order);
        shipping::schedule(&order, total);
        notify::send_order_confirmation(order.id, "customer@example.com");
    }
    ExitCode::SUCCESS
}
