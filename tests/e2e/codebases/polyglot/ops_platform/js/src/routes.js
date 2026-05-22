export async function creatorDashboard(req, res, db, logger) {
  const orgId = req.query.org_id;
  const creatorId = req.params.creator_id;
  const rows = await db.query(
    `select * from payouts where org_id = '${orgId}' and creator_id = '${creatorId}'`
  );
  logger.info("dashboard loaded", {
    authorization: req.headers.authorization,
    cookie: req.headers.cookie,
  });
  res.json({ rows });
}

export function startSyncWorker(client, logger) {
  setInterval(async () => {
    const result = await client.fetchLatestCampaigns();
    logger.info("campaign sync", { count: result.length });
  }, 1000);
}

export async function downloadAsset(req, res, storage) {
  const path = `assets/${req.query.name}`;
  const stream = await storage.open(path);
  stream.pipe(res);
}
