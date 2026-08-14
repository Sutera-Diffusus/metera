//! 每日邮件报告：SMTP 直发（lettre）。
//!
//! QQ/163 要求发件地址等于 SMTP 登录账号，因此发件人固定为配置的邮箱本身。
//! 所有发送共用同一 transport 构建：15s 超时、465=隐式 TLS / 其他=STARTTLS、错误不含授权码。

use std::time::Duration;

use lettre::message::{header::ContentType, Attachment, Mailbox, Message, MultiPart, SinglePart};
use lettre::transport::smtp::authentication::Credentials;
use lettre::{SmtpTransport, Transport};

/// 日报内联品牌图标（lobe-icons，640×640 PNG，编译期嵌入，CID 引用）。
/// HTML 中以 `cid:openai` 等形式引用；QQ/163 对内联附件直接显示，不弹「显示图片」。
const INLINE_ICONS: &[(&str, &[u8])] = &[
    ("openai", include_bytes!("../../assets/mail-icons/openai.png")),
    ("moonshot", include_bytes!("../../assets/mail-icons/moonshot.png")),
    ("deepseek", include_bytes!("../../assets/mail-icons/deepseek.png")),
    ("qwen", include_bytes!("../../assets/mail-icons/qwen.png")),
    ("claude", include_bytes!("../../assets/mail-icons/claude.png")),
];

fn sender_mailbox(email: &str) -> Result<Mailbox, String> {
    let mailbox: Mailbox = email
        .parse()
        .map_err(|error| format!("邮箱地址无效: {error}"))?;
    Ok(Mailbox::new(Some("Metera".into()), mailbox.email))
}

/// HTML 实际引用 `cid:` 的图标才随邮件嵌入——未引用的内联部分会被 QQ 等客户端当成 .bin 附件显示。
fn referenced_icons(html: &str) -> Vec<&'static (&'static str, &'static [u8])> {
    INLINE_ICONS
        .iter()
        .filter(|(cid, _)| html.contains(&format!("cid:{cid}")))
        .collect()
}

fn build_transport(
    email: &str,
    smtp_host: &str,
    smtp_port: u16,
    smtp_password: &str,
) -> Result<SmtpTransport, String> {
    let credentials = Credentials::new(email.to_string(), smtp_password.to_string());
    // 465 → 隐式 TLS（relay）；其他端口 → STARTTLS（starttls_relay，587 常见）
    let builder = if smtp_port == 465 {
        SmtpTransport::relay(smtp_host)
    } else {
        SmtpTransport::starttls_relay(smtp_host)
    }
    .map_err(|error| format!("SMTP 服务器配置无效: {error}"))?;
    Ok(builder
        .port(smtp_port)
        .credentials(credentials)
        .timeout(Some(Duration::from_secs(15)))
        .build())
}

fn send_plain(
    email: &str,
    smtp_host: &str,
    smtp_port: u16,
    smtp_password: &str,
    subject: &str,
    body: &str,
) -> Result<(), String> {
    let mailbox: Mailbox = email
        .parse()
        .map_err(|error| format!("邮箱地址无效: {error}"))?;
    let message = Message::builder()
        .from(sender_mailbox(email)?)
        .to(mailbox)
        .subject(subject)
        .header(ContentType::TEXT_PLAIN)
        .body(body.to_string())
        .map_err(|error| format!("构建邮件失败: {error}"))?;
    let transport = build_transport(email, smtp_host, smtp_port, smtp_password)?;
    transport
        .send(&message)
        .map(|_| ())
        .map_err(|error| format!("发送失败: {error}"))
}

/// 发送一封纯文本测试邮件到指定邮箱（发件人=收件人=email）。
/// 返回成功提示或可读错误；错误信息不含授权码。
pub fn send_test_email(
    email: &str,
    smtp_host: &str,
    smtp_port: u16,
    smtp_password: &str,
) -> Result<String, String> {
    send_plain(
        email,
        smtp_host,
        smtp_port,
        smtp_password,
        "Metera 测试邮件",
        "这是一封来自 Metera 的测试邮件。

收到它说明「每日邮件报告」的邮箱配置正确，可以放心开启。

—— Metera",
    )?;
    Ok(format!("测试邮件已发送到 {email}，请查收（也看看垃圾箱）"))
}

/// 发送每日报告邮件（主题与正文由 report 模块渲染）。
/// HTML+纯文本双格式（multipart/alternative），品牌图标走 multipart/related CID 内联。
pub fn send_report(
    email: &str,
    smtp_host: &str,
    smtp_port: u16,
    smtp_password: &str,
    subject: &str,
    body: &str,
    html: &str,
) -> Result<(), String> {
    let mailbox: Mailbox = email
        .parse()
        .map_err(|error| format!("邮箱地址无效: {error}"))?;
    let mut related = MultiPart::related().singlepart(SinglePart::html(html.to_string()));
    let png: ContentType = "image/png".parse().map_err(|error| format!("构建邮件失败: {error}"))?;
    for (cid, bytes) in referenced_icons(html) {
        related = related.singlepart(
            Attachment::new_inline(cid.to_string()).body(bytes.to_vec(), png.clone()),
        );
    }
    let message = Message::builder()
        .from(sender_mailbox(email)?)
        .to(mailbox)
        .subject(subject)
        .multipart(
            MultiPart::mixed().multipart(
                MultiPart::alternative()
                    .singlepart(SinglePart::plain(body.to_string()))
                    .multipart(related),
            ),
        )
        .map_err(|error| format!("构建邮件失败: {error}"))?;
    let transport = build_transport(email, smtp_host, smtp_port, smtp_password)?;
    transport
        .send(&message)
        .map(|_| ())
        .map_err(|error| format!("发送失败: {error}"))
}

#[cfg(test)]
mod tests {
    use super::{referenced_icons, send_report, send_test_email};

    /// 只嵌入 HTML 实际引用的图标（否则 QQ 把未引用的内联部分显示成 .bin 附件）。
    #[test]
    fn only_referenced_icons_are_embedded() {
        let icons = referenced_icons(r#"<img src="cid:openai"><img src="cid:moonshot">"#);
        let cids: Vec<&str> = icons.iter().map(|(cid, _)| *cid).collect();
        assert_eq!(cids, ["openai", "moonshot"]);
        assert!(referenced_icons("<p>无图标</p>").is_empty());
    }

    /// 不真正发信；仅验证参数校验路径不 panic、错误信息不含密码。
    #[test]
    fn invalid_email_is_rejected_without_password_leak() {
        let result = send_test_email("not-an-email", "smtp.qq.com", 465, "test-auth-code");
        let message = result.unwrap_err();
        assert!(message.contains("邮箱"), "应提示邮箱问题: {message}");
        assert!(!message.contains("secret"), "错误信息不得泄露授权码: {message}");
    }

    /// 报告发送同样不得泄露授权码。
    #[test]
    fn report_error_never_contains_password() {
        let result = send_report("not-an-email", "smtp.qq.com", 465, "test-auth-code", "主题", "正文", "<p>正文</p>");
        let message = result.unwrap_err();
        assert!(!message.contains("secret"), "错误信息不得泄露授权码: {message}");
    }

    /// 空主机名（builder 不做解析）应得到可读错误，同样不 panic。
    #[test]
    fn empty_host_does_not_panic() {
        let _ = send_test_email("user@qq.com", "", 465, "x");
        let _ = send_report("user@qq.com", "", 465, "x", "s", "b", "<p>b</p>");
    }

    /// 真实发送冒烟测试：凭据走环境变量（METERA_TEST_EMAIL / METERA_TEST_SMTP_HOST /
    /// METERA_TEST_SMTP_PORT / METERA_TEST_SMTP_PASSWORD），默认忽略，不进代码库。
    #[test]
    #[ignore]
    fn live_send_with_env_credentials() {
        let email = std::env::var("METERA_TEST_EMAIL").expect("METERA_TEST_EMAIL");
        let host = std::env::var("METERA_TEST_SMTP_HOST").expect("METERA_TEST_SMTP_HOST");
        let port: u16 = std::env::var("METERA_TEST_SMTP_PORT")
            .unwrap_or_else(|_| "465".into())
            .parse()
            .expect("METERA_TEST_SMTP_PORT 必须是端口号");
        let password = std::env::var("METERA_TEST_SMTP_PASSWORD").expect("METERA_TEST_SMTP_PASSWORD");
        let result = send_test_email(&email, &host, port, &password);
        assert!(result.is_ok(), "真实发送失败: {:?}", result.err());
    }
}
