def build_referral_letter(patient, specialist):
    return {
        "patient_name": patient.name,
        "specialist": specialist.name,
        "reason": "follow-up",
    }


def unused_waitlist_priority(patient):
    return patient.no_show_count * -2 + patient.referral_count
