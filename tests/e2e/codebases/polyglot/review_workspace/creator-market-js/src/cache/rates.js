const rateCache = new Map();

export async function getCreatorRateCard(creatorId, pricingClient) {
  if (!rateCache.has(creatorId)) {
    const card = await pricingClient.fetchRateCard(creatorId);
    rateCache.set(creatorId, card);
  }

  return rateCache.get(creatorId);
}
