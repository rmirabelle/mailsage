# Google sign-in setup for Gmail

Gmail does not accept a plain password from a mail program. Mail Sage must be
registered with Google as an app, and you must own that registration. You do
this one time. It is free.

Google changes the names of these pages from time to time. If a name here is
different on your screen, look for the nearest match.

## Steps

1. Go to <https://console.cloud.google.com/> and sign in with your Google account.
2. Create a project. Name it `Mail Sage`.
3. Open **APIs & Services → Library**, find **Gmail API**, and select **Enable**.
4. Open **Google Auth Platform** (older name: **OAuth consent screen**).
   - **Branding**: app name `Mail Sage`, and your email address for the support
     and developer contact fields.
   - **Audience**: user type **External**.
   - **Data access**: add the scope `https://mail.google.com/`.
5. Open **Clients** (older name: **Credentials**) and create an **OAuth client ID**.
   - Application type: **Desktop app**.
   - Name: `Mail Sage`.
6. Google shows a **Client ID** and a **Client secret**. Put them in
   `secrets/oauth.env`:

   ```
   GOOGLE_CLIENT_ID=<the client ID>
   GOOGLE_CLIENT_SECRET=<the client secret>
   ```

7. Build Mail Sage again. The build puts the two values into the app in a
   hidden form (not as plain text).

`secrets/oauth.env` is not in the Git repository. Do not put the two values in
a source file, a commit, or a chat.

## Testing status and the 7-day limit

A new registration has the publishing status **Testing**. In this status:

- Only the Google accounts in the **Test users** list can sign in. Add your
  Gmail address there (**Audience → Test users**).
- Google cancels the sign-in after 7 days. Mail Sage then tells you to sign
  in again.

The simplest way around the 7-day limit is a Google app password, which does
not expire:

1. Turn on 2-Step Verification for your Google account.
2. Open <https://myaccount.google.com/apppasswords>, type a name (for example
   Mail Sage), and copy the 16 letters.
3. In Mail Sage, select **File → Add account → Gmail with app password**.
   Type your address and the app password. If the address is already in Mail
   Sage, the account changes to the app password and keeps its folders.

Google cancels all app passwords when you change your Google password. Then
make a new one and add it the same way.

You can also select **Publish app** on the **Audience** page.
The status becomes **In production**. Google does not review the app until you
ask for it. Without the review, the sign-in page shows a warning that Google
has not verified the app. Because the app is yours, select **Advanced**, then
**Go to Mail Sage**.

## What Mail Sage does with the sign-in

- The sign-in page opens in your default browser. Mail Sage never sees your
  Google password.
- Mail Sage keeps one long-term token for each account in Windows Credential
  Manager (entry name `MailSage/account/<number>`). It is not in the database
  or in any file.
- When you remove the account in Mail Sage, Mail Sage deletes the token and
  tells Google to cancel the permission.
- You can also cancel the permission at <https://myaccount.google.com/permissions>.
