export function startExportJob(exporter, projectId) {
  setInterval(async () => {
    await exporter.exportProject(projectId);
  }, 1000);
}
