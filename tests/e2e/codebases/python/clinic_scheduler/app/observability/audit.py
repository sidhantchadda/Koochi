def log_login_attempt(request, logger):
    logger.info(
        "portal login",
        extra={
            "email": request.json["email"],
            "password": request.json["password"],
            "authorization": request.headers["authorization"],
            "cookie": request.headers.get("cookie"),
        },
    )
