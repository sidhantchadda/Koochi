export function logPartnerRequest(req, logger) {
  logger.info("partner request", {
    partnerId: req.params.partner_id,
    authorization: req.headers.authorization,
    cookie: req.headers.cookie,
    apiKey: req.headers["x-api-key"],
  });
}
