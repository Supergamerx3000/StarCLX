# Analyse STARFACE App für Windows 10.0.2.7

Stand: 30.09.2026. Quelle: `STARFACE_v10.0.2.7_x64.msi` (von Moe bereitgestellt), entpackt mit `msiextract` und dekompiliert mit ILSpy 9.1. Ziel ist ausschließlich die Interoperabilität: Protokolle und Schnittstellen werden abgeleitet, Code wird nicht übernommen.

## 1. Kurzfazit

Die Windows-App spricht **nicht** über die dokumentierte REST-API und **nicht** über die klassische UCI mit der Anlage. Der Kern ist eine **gRPC-API namens „OneHub“ auf Port 9092**. Darüber laufen Anrufsteuerung, Anrufliste, Präsenz, Voicemail, Fax, Umleitungen, Gruppen, Konferenzen, Favoriten, Transkription und auch die Vergabe der SIP-Zugangsdaten. Dazu kommen nur zwei weitere Kanäle: **SIP über TLS** für das Audio des Softphones und **XMPP** für den Chat.

Die Protokolldefinitionen sind als Protobuf-Deskriptoren in `OneHub.Client.Csharp.Internal.dll` eingebettet. Daraus wurden **95 `.proto`-Dateien mit 304 RPCs** rekonstruiert. Sie kompilieren fehlerfrei mit `protoc`: [proto/](../proto/).

Für den Linux-Nachbau heißt das: Wir können mit generierten gRPC-Stubs (für Rust, Go, Python, C++ usw.) genau die Schnittstelle nutzen, die der offizielle Client nutzt. Die Unsicherheit rund um UCI entfällt damit.

## 2. Architektur des Windows-Clients

| Baustein | Technik | Funktion |
|---|---|---|
| UI | .NET 10, WPF, ReactiveUI | Oberfläche, Workspaces, Widgets |
| Anlagen-API | gRPC über HTTPS, `Grpc.Net.Client`, Port 9092 | Nahezu alle Funktionen |
| Login | OpenID Connect (`IdentityModel.OidcClient`) | OAuth2 Authorization Code + PKCE |
| Softphone | Eigenes PJSIP-Wrapper-DLL (`SipUserAgentAdaptor.dll`, SWIG), PJSIP 2.17 | SIP-Registrierung und Audio |
| Chat | Smack (Java-XMPP-Bibliothek), per IKVM nach .NET übersetzt | Chat, Gruppenchat, Dateitransfer, Chatverlauf |
| Lokaler Speicher | SQLCipher | Verschlüsselter Chatverlauf |
| Video | `STARFACE-NEON.exe` (Electron-Unterordner `neonapp`) | NEON-Meetings |
| Sonstiges | Jabra-, Poly-, EPOS-, Yealink-SDKs; Busylight-SDK; RTSP/H.264 für Türkameras; TAPI-Anbindung über CoreWCF; Outlook-IM-Provider; Sentry, OpenTelemetry | Windows-spezifische Integrationen |

Die im Paket enthaltene `uci-all.dll` (Java-UCI per IKVM) wird nur noch für Hilfsumwandlungen genutzt. Die Anrufsteuerung läuft vollständig über gRPC.

## 3. Login (OAuth2 / OIDC)

- Authority ist `https://<anlage>[:webport]/`. Die Discovery liegt unter `/.well-known/openid-configuration`.
- Das Discovery-Dokument liefert zusätzlich ein Feld `edgeNodeId`. Ist es gesetzt, hängt der Client bei allen Token-Anfragen `resource=edgenode://<edgeNodeId>` an.
- Die Client-ID ist `windows-app` und die Redirect-URI `starface-app://login` (ein eigenes URL-Schema). Der Browser öffnet den Login, und das Schema leitet den Code zurück an die App.
- Als Fallback gibt es einen Password-Grant (`grant_type=password&client_id=windows-app&username=…&password=…`). Er funktioniert nur, wenn der Benutzer das Recht „API access with Password Grant“ hat.
- Refresh- und Revoke-Aufrufe laufen ebenfalls über `client_id=windows-app`.
- Das Access-Token wird für drei Zwecke verwendet:
  1. als `Authorization: Bearer <token>` für jeden gRPC-Aufruf,
  2. als **XMPP-Passwort**,
  3. für REST.
- Der User-Agent lautet `STARFACE (win) 10.0.2.7 (<OS-Version>; <.NET-Version>; x64)`.

**Offene Frage für Linux:** Wir brauchen eine eigene Client-ID mit Redirect auf `http://127.0.0.1:<port>/`. Die REST-Spezifikation erlaubt diesen Redirect ausdrücklich, aber es ist nicht klar, ob der Client `windows-app` Loopback-Redirects akzeptiert. Den Client `windows-app` weiterzuverwenden wäre technisch der einfachste Weg; wie sauber das ist, sollte Moe entscheiden. Unter Linux lässt sich `starface-app://` zudem als `x-scheme-handler` registrieren.

## 4. gRPC-API „OneHub“ (Port 9092)

- Die Adresse ist `https://<anlage>:9092`, sofern die Einstellungen keinen eigenen `GrpcEndpointUrl` setzen (erlaubt sind die Formen `grpcs://` und `https://`).
- Die Anmeldung erfolgt pro Aufruf mit Bearer-Token.
- Ereignisse kommen über **Server-Streaming** (`Subscribe…Events`). Für jeden Bereich gibt es einen dauerhaften Stream, Polling ist nicht nötig.
- Die Pakete heißen `gamma.onehub.api.v1.*`, bei den KI-Teilen auch `group.starface.onehub.api.v1.*`.

Für den Client relevante Dienste (Auswahl, vollständig in `proto/proxy/` und `proto/client-gateway/`):

| Dienst | Wichtige RPCs | Client-Feature |
|---|---|---|
| `CallService` | PlaceCall, HangupCall, HoldCall, ResumeCall, ForwardCall, TransferCall, PlaceConsultationCall, TransferConsultationCall, GrabCall, AcceptCallElsewhere, RecordCall, SwitchPhone, ParkAndOrbit, SendDtmf, SubscribeCallEvents | Call Manager komplett |
| `ConferenceCallService` | InitiateConference, JoinToConference, Mute, TransferToConference, … | Spontane Konferenzen |
| `ConferenceService` | Create/Update/Start/DeleteConference | Geplante Konferenzen |
| `JournalService` | GetJournal, GetJournalUpdates, SetJournalEntryComment, SetJournalEntryCalledBack, CallBack, SubscribeJournalEvents | Anrufliste mit Kommentaren |
| `PresenceService` | SubscribePresenceStates, GetPresenceDetails, SubscribePresenceEvents | Telefoniestatus der Kollegen, BLF |
| `MeService` | GetUser, GetPhones, Set/GetPrimaryPhone, SetDoNotDisturb, SetChatPresenceMessage, Set/GetCallWaitingIndication, SetSignalingPhoneNumber, UpdateAvatar, GetPermissions, SubscribeMeEvents | Eigenes Profil und Einstellungen |
| `SipDeviceService` | **RegisterSipDevice → SIP-Benutzer, Passwort, Realm, Port** | Softphone-Zugangsdaten |
| `ChatService` | GetChatId, GetUserId | Zuordnung von Jabber-ID und Benutzer |
| `VoiceMailService` | GetVoicemails, DownloadVoicemailFile (Stream), Delete/Move, CallVoicemailViaPhone, TransferCallToVoicemailBox | Voicemail |
| `FaxService` | GetFaxes, SendFax (Client-Stream), DownloadFaxFile, SubscribeFaxEvents | Fax |
| `RedirectService` | GetRedirects, UpdateRedirect, Enable/DisableRedirect, SubscribeRedirectEvents | Umleitungen |
| `GroupService` | GetGroupMemberships, LogOnToGroup, LogOffFromGroup | Gruppen und Queue-Login |
| `QueueService` | GetQueues, GetQueueCalls, GetQueueStatistics, GrabQueueCall | iQueue und Vermittlungsplatz |
| `FavoriteService` | CRUD für Favoriten und Favoritengruppen | VIP-Favoriten (serverseitig) |
| `ContactService`, `SimpleContactService` | GetContacts, CRUD, GetScheme, GetFolders | Adressbuch |
| `UserService` | GetUsers, SearchUsers, GetAvatar (Stream) | Kollegenliste |
| `FmcPhoneService`, `ModuleService`, `CallBackOnBusyService` | iFMC, Module, Rückruf bei besetzt | Funktionen-Workspace |
| `AudioRecordService` | GetAudioRecords, DownloadAudioRecordFile | Aufzeichnungen |
| `TranscriptionService`, `TranscriptionSessionService`, `SummaryTaskService` | Transkription, KI-Zusammenfassung, Aufgaben | KI-Funktionen |
| `NeonService` | GetNeonToken, Einladungen, Warteraum | Video-Meetings |
| `SystemService` | GetServerVersion, GetOAuthConfiguration, GetWebServerConfiguration | Start und Konfiguration |
| `CapabilityService`, `FeatureFlagsService`, `HealthService` | Fähigkeiten, Feature-Flags, Health-Check | Steuerung |

Die Dienste unter `proto/edge-node-gateway/`, `identity-service/` und `media-gateway/` sind server- bzw. Cloud-intern und für den Client nicht nötig.

## 5. Softphone (SIP)

- Die SIP-Zugangsdaten kommen aus `SipDeviceService.RegisterSipDevice`. Der Aufruf übergibt eine feste Geräte-ID `163C00A2-C2F1-4FFE-9474-49283C379852` und die App-Version. Die Antwort enthält `sip_user_id`, `password`, `realm` und `port`.
- Registriert wird am Anlagen-Host mit **TLS**, einem Registrierungs-Timeout von 3600 s und Re-INVITE/UPDATE bei IP-Wechsel. Die SIP-Zertifikate prüft die App selbst.
- **Die Anrufsteuerung läuft nicht über SIP.** Ausgehende Anrufe startet die App per gRPC `PlaceCall(number, phone_id)`. Die Anlage ruft dann das Softphone an, und dieses nimmt automatisch an (`SetAutoAnswerCalls(true)`). Halten, Übergabe und Konferenz laufen ebenfalls per gRPC. Das Softphone ist also ein reines Audio-Endgerät, ähnlich wie ein per CTI gesteuertes Tischtelefon.
- Codecs sind Opus, G.722, A-Law, µ-Law und GSM, die Prioritäten sind einstellbar. SRTP, STUN, BLF-/MWI-NOTIFY und lokale Aufzeichnung sind im Wrapper vorhanden.

Für Linux reicht deshalb ein schlanker SIP-Stack mit TLS-Registrierung, Auto-Answer und gutem Audio, zum Beispiel PJSIP oder baresip.

## 6. Chat (XMPP)

- Der Server ist Openfire auf `<anlage>:5222` mit **STARTTLS als Pflicht**. Die XMPP-Domain ist der Hostname der Anlage.
- Die eigene Jabber-ID kommt aus `ChatService.GetChatId` (gRPC). Benutzername ist deren lokaler Teil, **Passwort ist das OAuth-Access-Token** (SASL).
- Verwendete Erweiterungen sind Roster und Präsenz, Message Carbons (`urn:xmpp:carbons:2`), Delayed Delivery (`jabber:x:delay`), Serverarchiv nach XEP-0136 (`urn:xmpp:archive`) mit RSM-Paging, Multi-User-Chat und Dateitransfer.
- Der Chatverlauf wird lokal in SQLCipher gespeichert und mit dem Serverarchiv abgeglichen.

Unter Linux passt dazu jede XMPP-Bibliothek, etwa libstrophe, QXmpp, slixmpp oder xmpp-rs.

## 7. Konsequenzen für den Nachbau

1. **Schnittstellenwahl:** gRPC (OneHub) als Hauptschnittstelle, SIP/TLS für Audio und XMPP für Chat. Die REST-API ist nur ein Fallback. UCI wird nicht gebraucht.
2. **MVP-Reihenfolge:** OIDC-Login → gRPC-Verbindung mit Health-Check und Me-Service → RegisterSipDevice mit SIP-Registrierung → PlaceCall und SubscribeCallEvents → Journal → Presence → Chat.
3. **Wichtigstes Risiko:** Die gRPC-API ist intern, versioniert (`v1`) und nicht öffentlich. Sie kann sich mit Anlagen-Updates ändern. Die `.proto`-Dateien sollten deshalb je App-Version neu extrahiert werden; das Skript `proto/_protodump.py` automatisiert das.
4. **Offen, nur an einer echten Anlage prüfbar:** Akzeptiert die Anlage eine eigene OAuth-Client-ID bzw. Loopback-Redirects? Muss ein anderer Client eine eigene `SipDeviceId` bekommen? Welche Lizenz bzw. welches Recht braucht der Benutzer (`CheckLoginPermission`)?

## 8. Live-Test an einer STARFACE 10.0.2.6 (30.09.2026)

Getestet von einem Linux-Rechner (Zorin OS 18.1) mit Python, grpcio und aus den Protos erzeugten Stubs, baresip und slixmpp. Die Skripte liegen auf Moes Rechner unter `~/Dokumente/claude/starface-prototype/`.

| Schritt | Ergebnis |
|---|---|
| Ports 443, 9092, 5061, 5222 | offen (5060 abgelehnt) |
| OIDC-Discovery | `/.well-known/…` leitet weiter auf `/auth/realms/pbx/.well-known/openid-configuration`; Grants: password, authorization_code (PKCE S256), refresh_token, client_credentials; keine `edgeNodeId` |
| Password-Grant, `client_id=windows-app` | ✅ Access-Token 300 s, Refresh-Token 90 Tage |
| gRPC :9092 mit Bearer | ✅ Health, GetServerVersion (10.0.2.6), GetUser, GetPhones, GetPermissions. Keine Server-Reflection; der User-Agent wird offenbar nicht geprüft |
| RegisterSipDevice | ✅ legt das App-Telefon `SIP/1004.WinClient` an (phone_id 1002), Port 5061 |
| SIP-Registrierung (baresip, TLS) | ✅ 200 OK |
| Medien | **SRTP ist Pflicht** (SDES, RTP/SAVP, AES_CM_128_HMAC_SHA1_80); Codecs G.722, PCMA, PCMU |
| PlaceCall + SubscribeCallEvents | ✅ Die Anlage ruft das Softphone an, Auto-Answer, Audio kommt an. Events: callCreated → callProvisionalChanged ×3 → callDisconnected |
| XMPP | ✅ GetChatId liefert `<nst>@<host>`, Login mit STARTTLS und dem Token als Passwort, Roster geladen |
| Browser-Login (Auth Code + PKCE S256) | ✅ nur mit `client_id=windows-app`, Redirect `starface-app://login` (Loopback `127.0.0.1` wird abgelehnt) und Scope `pbx-login` (`openid` wird abgelehnt). Unter Linux nimmt ein `x-scheme-handler/starface-app` (.desktop-Datei) den Code entgegen. Beim ersten Browser-Login verlangt die Anlage eine Passwortänderung. Danach laufen gRPC und XMPP mit diesem Token; die Erneuerung per Refresh-Token (90 Tage) funktioniert |
