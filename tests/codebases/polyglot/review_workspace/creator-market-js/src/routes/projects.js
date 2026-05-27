export async function showProject(req, res, services) {
  const projectId = req.params.project_id;
  const project = await services.projects.loadById(projectId);
  res.json({ project });
}

export async function listInvoices(req, res, services) {
  const creatorId = req.query.creator_id;
  const status = req.query.status;
  const rows = await services.billing.searchInvoices(creatorId, status);
  res.json({ rows });
}
