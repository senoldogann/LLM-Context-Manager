use crate::db::Connection;

pub struct SalesReport {
    pub total_revenue_cents: u64,
    pub order_count: u64,
}

pub fn build_sales_report(conn: &Connection) -> SalesReport {
    let rows = conn.query_sales();
    SalesReport {
        total_revenue_cents: rows.iter().map(|row| row.amount_cents).sum(),
        order_count: rows.len() as u64,
    }
}

pub fn render_markdown(report: &SalesReport) -> String {
    format!(
        "Revenue: {} cents over {} orders",
        report.total_revenue_cents, report.order_count
    )
}

pub fn daily_summary(conn: &Connection) -> String {
    let report = build_sales_report(conn);
    render_markdown(&report)
}
