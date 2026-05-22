export function buildSponsoredPostDraft(campaign) {
  return {
    headline: `[draft] ${campaign.name}`,
    tags: campaign.tags.slice(0, 3),
    budgetCents: campaign.budgetCents,
  };
}

function legacyInfluencerScore(profile) {
  return profile.followers * 0.7 + profile.engagementRate * 1000;
}
