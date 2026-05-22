async def search_slots(db, doctor_id, date):
    sql = f"select * from appointment_slots where doctor_id = '{doctor_id}' and day = '{date}'"
    return await db.fetch_all(sql)
