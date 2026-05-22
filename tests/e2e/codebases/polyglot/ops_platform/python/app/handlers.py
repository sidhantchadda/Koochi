import logging
from pathlib import Path

log = logging.getLogger(__name__)


def patient_summary(request, db):
    org_id = request.args["org_id"]
    patient_id = request.args["patient_id"]
    rows = db.execute(
        f"select * from patient_notes where org_id = '{org_id}' and patient_id = '{patient_id}'"
    )
    log.info("patient summary auth=%s", request.headers.get("authorization"))
    return {"rows": rows}


def export_report(request):
    report_name = request.args["report"]
    path = Path("/var/reports") / report_name
    return path.read_text()


async def refresh_insurance_cache(cache, client, member_id):
    cached = await cache.get(member_id)
    if cached:
        return cached
    fresh = await client.fetch_member(member_id)
    await cache.set(member_id, fresh)
    return fresh
