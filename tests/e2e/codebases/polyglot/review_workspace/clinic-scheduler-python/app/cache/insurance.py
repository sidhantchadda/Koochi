insurance_cache = {}


async def insurance_profile(member_id, insurance_client):
    if member_id not in insurance_cache:
        insurance_cache[member_id] = await insurance_client.fetch_profile(member_id)

    return insurance_cache[member_id]
