from pathlib import Path


def read_export(report_name):
    return Path("/srv/clinic/reports").joinpath(report_name).read_text()
