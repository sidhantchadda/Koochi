async def get_patient_chart(request, repository):
    org_id = request.path_params["org_id"]
    patient_id = request.path_params["patient_id"]
    chart = await repository.fetch_chart(org_id, patient_id)
    return {"chart": chart}


async def find_open_slots(request, repository):
    doctor_id = request.query_params["doctor_id"]
    date = request.query_params["date"]
    return await repository.search_slots(doctor_id, date)
