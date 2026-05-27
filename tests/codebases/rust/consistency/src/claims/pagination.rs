const MAX_CLAIM_PAGE_SIZE: usize = 100;
pub fn bounded_claim_page(requested: usize) -> usize {
    requested.clamp(1, MAX_CLAIM_PAGE_SIZE)
}
pub fn unbounded_claim_page(requested: usize) -> usize {
    requested
}
