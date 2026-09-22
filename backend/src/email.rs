use lettre::message::header::ContentType;
use lettre::message::{MultiPart, SinglePart};
use lettre::transport::smtp::authentication::Credentials;
use lettre::transport::smtp::client::{Tls, TlsParameters};
use lettre::transport::smtp::extension::ClientId;
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};
use serde_json::json;
use std::time::Duration;

pub async fn send_verification_email(to_email: &str, code: &str) -> Result<(), String> {
    println!("[SIGNAL AUTH] Отправка кода верификации на: {}", to_email);

    if let Ok(brevo_key) = std::env::var("BREVO_API_KEY") {
        if !brevo_key.trim().is_empty() {
            return send_via_brevo_api(&brevo_key, to_email, code).await;
        }
    }

    if let Ok(resend_key) = std::env::var("RESEND_API_KEY") {
        if !resend_key.trim().is_empty() {
            match send_via_resend_api(&resend_key, to_email, code).await {
                Ok(_) => return Ok(()),
                Err(e) => {
                    eprintln!("[SIGNAL AUTH] Resend failed: {}", e);
                }
            }
        }
    }

    send_via_smtp(to_email, code).await
}

async fn send_via_brevo_api(api_key: &str, to_email: &str, code: &str) -> Result<(), String> {
    let client = reqwest::Client::new();
    let sender_email = std::env::var("SMTP_FROM").unwrap_or_else(|_| "noreply@signal-audio.io".to_string());

    let payload = json!({
        "sender": { "name": "SIGNAL", "email": sender_email },
        "to": [{ "email": to_email }],
        "subject": format!("Код подтверждения SIGNAL: {}", code),
        "htmlContent": format!(
            "<div style='font-family:sans-serif;background:#09090b;color:#fff;padding:30px;text-align:center;'>\
            <h2>SIGNAL AUDIO</h2>\
            <p>Ваш код подтверждения:</p>\
            <h1 style='color:#38bdf8;letter-spacing:6px;'>{}</h1>\
            <p style='color:#71717a;font-size:12px;'>Код действует 15 минут.</p>\
            </div>",
            code
        )
    });

    let res = client
        .post("https://api.brevo.com/v3/smtp/email")
        .header("api-key", api_key)
        .header("Content-Type", "application/json")
        .body(payload.to_string())
        .send()
        .await
        .map_err(|e| format!("HTTP request failed: {}", e))?;

    if !res.status().is_success() {
        let err_body = res.text().await.unwrap_or_default();
        return Err(format!("Brevo API error: {}", err_body));
    }

    Ok(())
}

async fn send_via_resend_api(api_key: &str, to_email: &str, code: &str) -> Result<(), String> {
    let client = reqwest::Client::new();
    let sender = std::env::var("RESEND_FROM").unwrap_or_else(|_| "SIGNAL <onboarding@resend.dev>".to_string());

    let payload = json!({
        "from": sender,
        "to": [to_email],
        "subject": format!("Код подтверждения SIGNAL: {}", code),
        "html": format!(
            "<div style='font-family:sans-serif;background:#09090b;color:#fff;padding:30px;text-align:center;'>\
            <h2>SIGNAL AUDIO</h2>\
            <p>Ваш код подтверждения:</p>\
            <h1 style='color:#38bdf8;letter-spacing:6px;'>{}</h1>\
            <p style='color:#71717a;font-size:12px;'>Код действует 15 минут.</p>\
            </div>",
            code
        )
    });

    let res = client
        .post("https://api.resend.com/emails")
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .body(payload.to_string())
        .send()
        .await
        .map_err(|e| format!("HTTP request failed: {}", e))?;

    if !res.status().is_success() {
        let err_body = res.text().await.unwrap_or_default();
        return Err(format!("Resend API error: {}", err_body));
    }

    Ok(())
}

async fn send_via_smtp(to_email: &str, code: &str) -> Result<(), String> {
    let smtp_user = std::env::var("SMTP_USER").unwrap_or_default();
    let smtp_pass = std::env::var("SMTP_PASS").unwrap_or_default();

    if smtp_user.is_empty() || smtp_pass.is_empty() {
        return Err("На сервере не заданы переменные SMTP_USER и SMTP_PASS для отправки писем".to_string());
    }

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
                return Err("Укажите SMTP_HOST в переменных окружения".to_string());
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

    let text_part = SinglePart::builder()
        .header(ContentType::TEXT_PLAIN)
        .body(format!(
            "SIGNAL AUDIO // СИСТЕМА АВТОРИЗАЦИИ\n\n\
            Ваш 6-значный цифровой код подтверждения:\n\n\
            >>>  {}  <<<\n\n\
            Срок действия кода: 15 минут.\n\
            Если вы не запрашивали данный код, просто проигнорируйте это письмо.\n",
            code
        ));

    let html_part = SinglePart::builder()
        .header(ContentType::TEXT_HTML)
        .body(format!(
            r#"<!DOCTYPE html>
<html>
<head><meta charset="utf-8"></head>
<body style="margin:0;padding:24px;background-color:#09090b;font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',Roboto,Helvetica,Arial,sans-serif;color:#ffffff;">
  <div style="max-width:440px;margin:0 auto;background:#111114;border:1px solid #27272a;border-radius:12px;padding:32px;text-align:center;">
    <h2 style="margin:0 0 8px;font-size:20px;font-weight:700;color:#ffffff;letter-spacing:0.05em;">SIGNAL AUDIO</h2>
    <p style="margin:0 0 20px;font-size:13px;color:#a1a1aa;">Код для подтверждения регистрации</p>
    <div style="background:#18181b;border:1px solid #38bdf8;border-radius:8px;padding:16px;margin:20px 0;letter-spacing:0.35em;font-size:32px;font-weight:800;color:#38bdf8;font-family:monospace;">{}</div>
    <p style="margin:20px 0 0;font-size:12px;color:#71717a;">Код действует в течение 15 минут.<br>Если вы не запрашивали регистрацию, проигнорируйте это письмо.</p>
  </div>
</body>
</html>"#,
            code
        ));

    let email = Message::builder()
        .from(format!("SIGNAL <{}>", sender_email).parse().map_err(|e: lettre::address::AddressError| e.to_string())?)
        .to(to_email.parse().map_err(|e: lettre::address::AddressError| e.to_string())?)
        .subject(format!("Код подтверждения SIGNAL: {}", code))
        .multipart(MultiPart::alternative().singlepart(text_part).singlepart(html_part))
        .map_err(|e| e.to_string())?;

    let target_host = if let Ok(addrs) = tokio::net::lookup_host(format!("{}:{}", smtp_host, smtp_port)).await {
        if let Some(v4) = addrs.into_iter().find(|a| a.is_ipv4()) {
            v4.ip().to_string()
        } else {
            smtp_host.clone()
        }
    } else {
        smtp_host.clone()
    };

    let tls_params = TlsParameters::new(smtp_host.clone())
        .map_err(|e| format!("TLS error: {}", e))?;
    let tls = if smtp_port == 465 {
        Tls::Wrapper(tls_params)
    } else {
        Tls::Required(tls_params)
    };

    let mut transport_builder = AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&target_host)
        .port(smtp_port)
        .hello_name(ClientId::Domain(smtp_host.clone()))
        .tls(tls);

    if !smtp_user.is_empty() && !smtp_pass.is_empty() {
        transport_builder = transport_builder.credentials(Credentials::new(smtp_user, smtp_pass));
    }

    let transport = transport_builder.build();
    let send_fut = transport.send(email);
    match tokio::time::timeout(Duration::from_secs(12), send_fut).await {
        Ok(Ok(_)) => Ok(()),
        Ok(Err(e)) => {
            eprintln!("[SIGNAL AUTH] Ошибка отправки SMTP: {}", e);
            Err(format!("Ошибка доставки письма через SMTP: {}", e))
        }
        Err(_) => {
            eprintln!("[SIGNAL AUTH] SMTP timeout");
            Err("Таймаут подключения к SMTP серверу".to_string())
        }
    }
}
