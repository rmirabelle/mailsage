# Microsoft sign-in setup for Outlook.com and Hotmail

Outlook.com and Hotmail do not accept a plain password from a mail program.
Mail Sage must be registered with Microsoft as an app, and you must own that
registration. You do this one time. It is free.

Microsoft changes the names of these pages from time to time. If a name here
is different on your screen, look for the nearest match.

## Steps

1. Go to <https://entra.microsoft.com/> and sign in with your Microsoft
   account. If Microsoft says that the account has no directory (tenant),
   create a free Azure account at <https://azure.microsoft.com/free> first,
   then come back. The free account gives you a directory.
2. Open **Entra ID → App registrations** and select **New registration**.
   - Name: `Mail Sage`.
   - Supported account types: **Personal Microsoft accounts only**.
   - Redirect URI: platform **Public client/native (mobile & desktop)**, value
     `http://localhost`.
   - Select **Register**.
3. The **Overview** page shows the **Application (client) ID**. Put it in
   `secrets/oauth.env`:

   ```
   MICROSOFT_CLIENT_ID=<the application (client) ID>
   ```

4. Build Mail Sage again. The build puts the value into the app in a hidden
   form (not as plain text).

Do not create a client secret. Mail Sage is a desktop app, and Microsoft calls
that a "public client": it has an ID and no secret.

You do not have to add permissions on the **API permissions** page. Mail Sage
asks for them at the sign-in, and Microsoft shows them to you there:

- Read and write your mail (IMAP).
- Send mail as you (SMTP).
- Keep the access until you remove it.
- See your email address.

`secrets/oauth.env` is not in the Git repository.

## Work or school accounts (Microsoft 365)

A work or school account needs a second registration. Microsoft does not let
you change the first one from "personal accounts only".

1. In the Azure portal, open **App registrations** and select **New registration**.
   - Name: `Mail Sage (work)`.
   - Supported account types: **Accounts in any organizational directory
     (Any Microsoft Entra ID tenant - Multitenant)**.
   - Redirect URI: platform **Public client/native (mobile & desktop)**, value
     `http://localhost`.
   - Select **Register**.
2. Put the **Application (client) ID** of this registration in
   `secrets/oauth.env`:

   ```
   MICROSOFT_WORK_CLIENT_ID=<the application (client) ID>
   ```

3. Build Mail Sage again. The "Add account" dialog then has **Work or school
   (Microsoft 365)**.

The company decides what a mail program can do with its mailboxes:

- **"Need admin approval"** at the sign-in: the company does not let you
  approve a program that Microsoft has not verified. An administrator of the
  company must approve Mail Sage. Give the administrator the client ID.
- **The server refuses the sign-in after Microsoft accepted it**: IMAP is off
  for your mailbox. An administrator can turn it on.
- **Mail Sage reads mail but cannot send**: "authenticated SMTP" is off for
  your mailbox. An administrator can turn it on.

Mail Sage connects to `outlook.office365.com` (read mail, port 993) and
`smtp.office365.com` (send mail, port 587) for these accounts.

## If the sign-in or the connection fails

- **"unauthorized_client" or "not enabled for consumers"**: the registration
  does not permit personal accounts. Open **Authentication** (or **Manifest**)
  and set the supported account types to personal Microsoft accounts.
- **"redirect_uri is not valid"**: the redirect URI must be `http://localhost`
  exactly, on the platform **Mobile and desktop applications**.
- **The server refuses the sign-in after Microsoft accepted it**: go to
  <https://outlook.live.com/>, open **Settings → Mail → Forwarding and IMAP**
  (older name: **Sync email → POP and IMAP**), and let devices and apps use
  IMAP.

## What Mail Sage does with the sign-in

- The sign-in page opens in your default browser. Mail Sage never sees your
  Microsoft password.
- Mail Sage keeps one long-term token for each account in Windows Credential
  Manager (entry name `MailSage/account/<number>`). It is not in the database
  or in any file. Microsoft gives a new one from time to time; Mail Sage keeps
  only the newest.
- When you remove the account in Mail Sage, Mail Sage deletes the token.
  Microsoft has no command that lets an app cancel its permission, so cancel
  it yourself at <https://account.live.com/consent/Manage> if you want that.
- Mail Sage connects to `outlook.office365.com` (read mail, port 993) and
  `smtp-mail.outlook.com` (send mail, port 587). The two connections are
  encrypted.
