use lettre::message::header::ContentType;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};

pub async fn send_verification_email(to_email: &str, code: &str) -> Result<(), String> {
    println!("[SIGNAL AUTH] Код верификации для {}: {}", to_email, code);

    let smtp_user = std::env::var("SMTP_USER").unwrap_or_default();
    let smtp_pass = std::env::var("SMTP_PASS").unwrap_or_default();

    let smtp_host = match std::env::var("SMTP_HOST") {
        Ok(h) if !h.trim().is_empty() => h,
        _ => {
            if smtp_user.ends_with("@gmail.com") {
                "smtp.gmail.com".to_string()
            } else if smtp_user.ends_with("@yandex.ru") || smtp_user.ends_with("@yandex.com") {
                "smtp.yandex.ru".to_string()
            } else if smtp_user.ends_with("@mail.ru") {
                "smtp.mail.ru".to_string()
            } else {
                return Ok(());
            }
        }
    };

    let smtp_port: u16 = std::env::var("SMTP_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or_else(|| {
            if smtp_host.contains("gmail") {
                587
            } else {
                465
            }
        });

    let sender_email = std::env::var("SMTP_FROM").unwrap_or_else(|_| {
        if !smtp_user.is_empty() && smtp_user.contains('@') {
            smtp_user.clone()
        } else {
            "noreply@signal-audio.io".to_string()
        }
    });

    let email_body = format!(
        "SIGNAL AUDIO // СИСТЕМА АВТОРИЗАЦИИ\n\n\
        Ваш 6-значный цифровой код подтверждения:\n\n\
        >>>  {}  <<<\n\n\
        Срок действия кода: 15 минут.\n\
        Если вы не запрашивали этот код, просто проигнорируйте это письмо.\n",
        code
    );

    let email = Message::builder()
        .from(sender_email.parse().map_err(|e: lettre::address::AddressError| e.to_string())?)
        .to(to_email.parse().map_err(|e: lettre::address::AddressError| e.to_string())?)
        .subject(format!("SIGNAL // Код подтверждения: {}", code))
        .header(ContentType::TEXT_PLAIN)
        .body(email_body)
        .map_err(|e| e.to_string())?;

    let mut transport_builder = AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&smtp_host)
        .port(smtp_port);

    if !smtp_user.is_empty() && !smtp_pass.is_empty() {
        transport_builder = transport_builder.credentials(Credentials::new(smtp_user, smtp_pass));
    }

    let transport = transport_builder.build();
    if let Err(e) = transport.send(email).await {
        eprintln!("[SIGNAL AUTH] Ошибка отправки SMTP: {}", e);
    }

    Ok(())
}
