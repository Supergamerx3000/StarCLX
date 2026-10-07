/*
 * Dünne Schicht zwischen Rust und libbaresip.
 *
 * baresip/re sind nicht thread-sicher: alles läuft auf dem Thread, der
 * sfsip_run() aufruft. Andere Threads schicken Befehle über sfsip_cmd(),
 * die per mqueue in diesen Thread übergeben werden. Ereignisse gehen über
 * einen Callback zurück an Rust (ebenfalls auf dem baresip-Thread).
 */

#include <stdlib.h>
#include <string.h>
#include <re.h>
#include <baresip.h>
#include <re_dbg.h>

enum sfsip_op {
	SFSIP_OP_ADD_UA = 1,
	SFSIP_OP_ANSWER,
	SFSIP_OP_HANGUP,
	SFSIP_OP_MUTE,
	SFSIP_OP_UNMUTE,
	SFSIP_OP_DTMF,
	SFSIP_OP_CONNECT,
	SFSIP_OP_QUIT,
	SFSIP_OP_RESET,
	SFSIP_OP_AUSRC,
};

/* Stabile Ereigniscodes für Rust (unabhängig von enum bevent_ev) */
enum {
	SFSIP_EV_READY = 1,
	SFSIP_EV_ERROR,
	SFSIP_EV_REGISTER_OK,
	SFSIP_EV_REGISTER_FAIL,
	SFSIP_EV_CALL_INCOMING,
	SFSIP_EV_CALL_OUTGOING,
	SFSIP_EV_CALL_RINGING,
	SFSIP_EV_CALL_ESTABLISHED,
	SFSIP_EV_CALL_CLOSED,
	SFSIP_EV_CALL_MENC,
	SFSIP_EV_AUDIO_ERROR,
	SFSIP_EV_CALL_DTMF,
	SFSIP_EV_END_OF_FILE,
};

struct sfsip_event {
	int ev;
	const char *aor;
	const char *call_id;
	const char *peer_uri;
	const char *peer_name;
	const char *text;
	int answer_delay;
	int outgoing;
};

typedef void (sfsip_event_cb)(void *ctx, const struct sfsip_event *e);

struct sfcmd {
	char *a;
	char *b;
};

static struct mqueue *mq;
static sfsip_event_cb *event_cb;
static void *event_ctx;

static void emit(int ev, struct ua *ua, struct call *call, const char *text)
{
	struct sfsip_event e;

	memset(&e, 0, sizeof(e));
	e.ev = ev;
	e.text = text;
	e.answer_delay = -1;
	if (ua)
		e.aor = account_aor(ua_account(ua));
	if (call) {
		e.call_id = call_id(call);
		e.peer_uri = call_peeruri(call);
		e.peer_name = call_peername(call);
		e.answer_delay = call_answer_delay(call);
		e.outgoing = call_is_outgoing(call);
	}

	if (event_cb)
		event_cb(event_ctx, &e);
}

static void bevent_handler(enum bevent_ev ev, struct bevent *event, void *arg)
{
	int code;
	(void)arg;

	/* Eingehende INVITEs muss die Anwendung selbst annehmen (sonst tut es
	 * das menu-Modul, das wir nicht laden). Ob geklingelt oder
	 * abgenommen wird, entscheidet danach Rust. */
	if (ev == BEVENT_SIPSESS_CONN) {
		const struct sip_msg *msg = bevent_get_msg(event);
		struct ua *ua = uag_find_msg(msg);

		if (ua && !ua_accept(ua, msg))
			bevent_stop(event);
		return;
	}

	switch (ev) {
	case BEVENT_REGISTER_OK:      code = SFSIP_EV_REGISTER_OK;      break;
	case BEVENT_REGISTER_FAIL:    code = SFSIP_EV_REGISTER_FAIL;    break;
	case BEVENT_CALL_INCOMING:    code = SFSIP_EV_CALL_INCOMING;    break;
	case BEVENT_CALL_OUTGOING:    code = SFSIP_EV_CALL_OUTGOING;    break;
	case BEVENT_CALL_RINGING:     code = SFSIP_EV_CALL_RINGING;     break;
	case BEVENT_CALL_ESTABLISHED: code = SFSIP_EV_CALL_ESTABLISHED; break;
	case BEVENT_CALL_CLOSED:      code = SFSIP_EV_CALL_CLOSED;      break;
	case BEVENT_CALL_MENC:        code = SFSIP_EV_CALL_MENC;        break;
	case BEVENT_AUDIO_ERROR:      code = SFSIP_EV_AUDIO_ERROR;      break;
	case BEVENT_CALL_DTMF_START:  code = SFSIP_EV_CALL_DTMF;        break;
	case BEVENT_END_OF_FILE:      code = SFSIP_EV_END_OF_FILE;      break;
	default:
		return;
	}

	emit(code, bevent_get_ua(event), bevent_get_call(event),
	     bevent_get_text(event));
}

/* Sucht einen Anruf nach SIP-Call-ID. Mit `incoming_only` nur eingehende;
 * das zählt, wenn beide Seiten eines Anrufs im selben Prozess laufen. */
static struct ua *find_ua_for_call(const char *id, bool incoming_only,
				   struct call **callp)
{
	struct le *le;

	for (le = list_head(uag_list()); le; le = le->next) {
		struct ua *ua = le->data;
		struct le *lc;

		for (lc = list_head(ua_calls(ua)); lc; lc = lc->next) {
			struct call *call = lc->data;

			if (incoming_only && call_is_outgoing(call))
				continue;

			if (!id || 0 == str_cmp(call_id(call), id)) {
				*callp = call;
				return ua;
			}
		}
	}

	return NULL;
}

static void mqueue_handler(int id, void *data, void *arg)
{
	struct sfcmd *c = data;
	struct ua *ua = NULL;
	struct call *call = NULL;
	int err = 0;
	(void)arg;

	switch (id) {

	case SFSIP_OP_ADD_UA:
		err = ua_alloc(&ua, c->a);
		if (!err && account_regint(ua_account(ua)))
			err = ua_register(ua);
		if (err)
			emit(SFSIP_EV_ERROR, ua, NULL, "ua_alloc/register");
		break;

	case SFSIP_OP_ANSWER:
		ua = find_ua_for_call(c->a, true, &call);
		if (ua)
			err = ua_answer(ua, call, VIDMODE_OFF);
		break;

	case SFSIP_OP_HANGUP:
		ua = find_ua_for_call(c->a, false, &call);
		if (ua)
			ua_hangup(ua, call, 0, NULL);
		break;

	case SFSIP_OP_MUTE:
	case SFSIP_OP_UNMUTE:
		ua = find_ua_for_call(c->a, false, &call);
		if (ua)
			audio_mute(call_audio(call), id == SFSIP_OP_MUTE);
		break;

	case SFSIP_OP_DTMF:
		ua = find_ua_for_call(c->a, false, &call);
		if (ua && c->b) {
			const char *p;
			for (p = c->b; *p && !err; p++)
				err = call_send_digit(call, *p);
			if (!err)
				err = call_send_digit(call, KEYCODE_REL);
		}
		break;

	case SFSIP_OP_AUSRC:
		/* b: "modul,gerät", z. B. "aufile,/tmp/ansage.wav" */
		ua = find_ua_for_call(c->a, false, &call);
		if (ua && c->b) {
			char *dev = strchr(c->b, ',');

			if (dev)
				*dev++ = '\0';
			err = audio_set_source(call_audio(call), c->b, dev);
		}
		break;

	case SFSIP_OP_CONNECT:
		ua = uag_find_aor(c->a);
		if (ua)
			err = ua_connect(ua, &call, NULL, c->b, VIDMODE_OFF);
		else
			err = ENOENT;
		if (err)
			emit(SFSIP_EV_ERROR, ua, NULL, "connect");
		break;

	case SFSIP_OP_RESET:
		/* Nach Standby oder Netzwechsel: SIP-Verbindungen neu aufbauen,
		 * neu registrieren und laufende Gespräche per re-INVITE umziehen. */
		err = uag_reset_transp(true, true);
		break;

	case SFSIP_OP_QUIT:
		ua_stop_all(false);
		re_cancel();
		break;
	}

	if (err && id != SFSIP_OP_ADD_UA && id != SFSIP_OP_CONNECT)
		emit(SFSIP_EV_ERROR, ua, call, "command failed");

	if (c) {
		free(c->a);
		free(c->b);
		free(c);
	}
}

/* Thread-sicher. Gibt 0 zurück, wenn der Befehl eingereiht wurde. */
int sfsip_cmd(int op, const char *a, const char *b)
{
	struct sfcmd *c;
	int err;

	if (!mq)
		return EAGAIN;

	c = calloc(1, sizeof(*c));
	if (!c)
		return ENOMEM;
	c->a = a ? strdup(a) : NULL;
	c->b = b ? strdup(b) : NULL;

	err = mqueue_push(mq, op, c);
	if (err) {
		free(c->a);
		free(c->b);
		free(c);
	}

	return err;
}

/*
 * Initialisiert baresip mit der Konfiguration in `config` und läuft bis
 * SFSIP_OP_QUIT. Blockiert; auf einem eigenen Thread aufrufen. Pro Prozess
 * nur einmal gleichzeitig.
 */
int sfsip_run(const char *config, const char *software,
	      sfsip_event_cb *cb, void *ctx)
{
	int err;

	event_cb = cb;
	event_ctx = ctx;

	err = libre_init();
	if (err)
		goto out;

	if (getenv("SFSIP_DEBUG")) {
		dbg_init(DBG_DEBUG, DBG_NONE);
		log_enable_debug(true);
	}
	else {
		dbg_init(DBG_WARNING, DBG_NONE);
		log_enable_stdout(false);
	}

	err = conf_configure_buf((const uint8_t *)config, strlen(config));
	if (err)
		goto out;

	re_thread_async_init(4);

	err = baresip_init(conf_config());
	if (err)
		goto out;

	err = ua_init(software, true, true, true);
	if (err)
		goto out;

	err = conf_modules();
	if (err)
		goto out;

	err = bevent_register(bevent_handler, NULL);
	if (err)
		goto out;

	err = mqueue_alloc(&mq, mqueue_handler, NULL);
	if (err)
		goto out;

	emit(SFSIP_EV_READY, NULL, NULL, NULL);

	err = re_main(NULL);

 out:
	if (err)
		emit(SFSIP_EV_ERROR, NULL, NULL, "init");

	mq = mem_deref(mq);
	bevent_unregister(bevent_handler);
	ua_stop_all(true);
	ua_close();
	module_app_unload();
	conf_close();
	baresip_close();
	mod_close();
	re_thread_async_close();
	libre_close();

	event_cb = NULL;
	event_ctx = NULL;

	return err;
}
