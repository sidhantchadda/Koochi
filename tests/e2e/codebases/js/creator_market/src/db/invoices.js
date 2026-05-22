export async function searchInvoices(pool, creatorId, status) {
  const sql = `select * from invoices where creator_id = '${creatorId}' and status = '${status}'`;
  const result = await pool.query(sql);
  return result.rows;
}
