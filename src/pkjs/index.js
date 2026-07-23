// Strength — phone-side relay (SPEC.md §3).
// Reassembles set recordings from the watch and POSTs them to the server as the
// labelled tuning corpus, authenticated with a per-device token.
//
// The device token and server are configured from the Pebble app's settings
// gear (see showConfiguration below) and stored in localStorage. Get a token at
// pebblestrength.app -> Devices -> Add device.

var DEFAULT_SERVER = 'https://pebblestrength.app';

var MSG_REC_META = 0;
var MSG_REC_CHUNK = 1;
var MSG_REC_DONE = 2;

var pending = {};

function getSettings() {
  var token = '';
  var server = DEFAULT_SERVER;
  try {
    token = localStorage.getItem('device_token') || '';
    server = localStorage.getItem('server') || DEFAULT_SERVER;
  } catch (e) { /* localStorage unavailable */ }
  return { token: token, server: server };
}

var B64 = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/';
function b64encode(bytes) {
  var out = '';
  for (var i = 0; i < bytes.length; i += 3) {
    var b0 = bytes[i], b1 = bytes[i + 1], b2 = bytes[i + 2];
    out += B64[b0 >> 2];
    out += B64[((b0 & 3) << 4) | ((b1 || 0) >> 4)];
    out += (i + 1 < bytes.length) ? B64[((b1 & 15) << 2) | ((b2 || 0) >> 6)] : '=';
    out += (i + 2 < bytes.length) ? B64[b2 & 63] : '=';
  }
  return out;
}

function b64decode(str) {
  var out = [];
  var buf = 0, bits = 0;
  for (var i = 0; i < str.length; i++) {
    var c = str.charAt(i);
    if (c === '=') break;
    var v = B64.indexOf(c);
    if (v < 0) continue;
    buf = (buf << 6) | v;
    bits += 6;
    if (bits >= 8) {
      bits -= 8;
      out.push((buf >> bits) & 0xff);
    }
  }
  return out;
}

// Workout download (server -> watch). Each packed workout fits one AppMessage,
// so send them one at a time (chained on ack), then a WK_DONE.
function sendWorkouts(slots, done) {
  done = done || function () {};
  var i = 0;
  function next() {
    if (i >= slots.length) {
      Pebble.sendAppMessage({ WK_DONE: 1 },
        function() { console.log('workout sync sent ' + slots.length); done(); },
        function() { console.log('WK_DONE send failed'); done(); });
      return;
    }
    var bytes = b64decode(slots[i].data);
    Pebble.sendAppMessage(
      { WK_TOTAL: slots.length, WK_INDEX: i, WK_DATA: bytes },
      function() { i++; next(); },
      function() { console.log('workout ' + i + ' send failed, stopping'); done(); }
    );
  }
  next();
}

// The AppMessage outbox is serial, so counter sync is chained to run only after
// workouts finish (`done`) rather than racing them.
function syncWorkouts(done) {
  done = done || function () {};
  var s = getSettings();
  if (!s.token) { console.log('no device token — skipping sync'); done(); return; }
  var xhr = new XMLHttpRequest();
  xhr.open('GET', s.server + '/api/device/workouts');
  xhr.setRequestHeader('Authorization', 'Bearer ' + s.token);
  xhr.onload = function() {
    if (xhr.status !== 200) { console.log('workout sync HTTP ' + xhr.status); done(); return; }
    try {
      var slots = (JSON.parse(xhr.responseText).slots) || [];
      if (!slots.length) { console.log('no assigned workouts to sync'); done(); return; }
      console.log('syncing ' + slots.length + ' workouts to watch');
      sendWorkouts(slots, done);
    } catch (err) { console.log('workout sync parse error: ' + err); done(); }
  };
  xhr.onerror = function() { console.log('workout sync failed (server unreachable)'); done(); };
  xhr.send();
}

function push16(b, v) { b.push(v & 255, (v >> 8) & 255); }

// One packed 14-byte record per counter config; must match counters_store.c.
function packCounters(cs) {
  var b = [];
  for (var i = 0; i < cs.length; i++) {
    var c = cs[i];
    b.push(c.movement_id & 255, (c.kind || 0) & 255, c.axis_mode & 255, c.thr_pct & 255);
    push16(b, c.lp_ms); push16(b, c.hp_ms); push16(b, c.min_rep_ms);
    push16(b, c.min_amp); push16(b, c.warmup_ms);
  }
  return b;
}

function syncCounters() {
  var s = getSettings();
  if (!s.token) { return; }
  var xhr = new XMLHttpRequest();
  xhr.open('GET', s.server + '/api/device/counters');
  xhr.setRequestHeader('Authorization', 'Bearer ' + s.token);
  xhr.onload = function() {
    if (xhr.status !== 200) { console.log('counter sync HTTP ' + xhr.status); return; }
    try {
      var cs = (JSON.parse(xhr.responseText).counters) || [];
      if (!cs.length) { console.log('no counter configs to sync'); return; }
      console.log('syncing ' + cs.length + ' counter configs to watch');
      Pebble.sendAppMessage(
        { CN_COUNT: cs.length, CN_DATA: packCounters(cs) },
        function() { console.log('counter sync sent ' + cs.length); },
        function() { console.log('counter sync send failed'); }
      );
    } catch (err) { console.log('counter sync parse error: ' + err); }
  };
  xhr.onerror = function() { console.log('counter sync failed (server unreachable)'); };
  xhr.send();
}

function upload(meta, actual, bytes) {
  var s = getSettings();
  if (!s.token) {
    console.log('no device token — open the Strength app settings to add one; recording not uploaded');
    return;
  }
  var body = JSON.stringify({
    movement_id: meta.MOVEMENT,
    workout_name: meta.WORKOUT_NAME || '',
    set_index: meta.SET_INDEX,
    actual: actual,
    is_timed: !!meta.TIMED,
    sample_rate: meta.RATE,
    sample_count: meta.SAMPLE_COUNT,
    truncated: !!meta.TRUNCATED,
    data: b64encode(bytes)
  });
  var xhr = new XMLHttpRequest();
  xhr.open('POST', s.server + '/api/device/recordings');
  xhr.setRequestHeader('Content-Type', 'application/json');
  xhr.setRequestHeader('Authorization', 'Bearer ' + s.token);
  xhr.onload = function() {
    if (xhr.status === 401) {
      console.log('recording upload unauthorised (401) — check the device token in settings');
    } else {
      console.log('recording upload: ' + xhr.status + ' ' + xhr.responseText);
    }
  };
  xhr.onerror = function() {
    console.log('recording upload failed (server unreachable at ' + s.server + ')');
  };
  xhr.send(body);
}

Pebble.addEventListener('ready', function() {
  var s = getSettings();
  console.log('Strength pkjs ready, server: ' + s.server +
              ', token: ' + (s.token ? 'set' : 'NOT SET — open app settings'));
  syncWorkouts(syncCounters);
});

// Settings gear -> open the config page, prefilled with current values.
Pebble.addEventListener('showConfiguration', function() {
  var s = getSettings();
  var url = s.server + '/watch/config?token=' + encodeURIComponent(s.token) +
            '&server=' + encodeURIComponent(s.server);
  Pebble.openURL(url);
});

// Config page closed -> persist the returned settings.
Pebble.addEventListener('webviewclosed', function(e) {
  if (!e || !e.response) return;
  try {
    var settings = JSON.parse(decodeURIComponent(e.response));
    if (typeof settings.token === 'string') localStorage.setItem('device_token', settings.token);
    if (settings.server) localStorage.setItem('server', settings.server);
    console.log('settings saved: server=' + (settings.server || DEFAULT_SERVER) +
                ', token=' + (settings.token ? 'set' : 'cleared'));
  } catch (err) {
    console.log('bad settings response: ' + err);
  }
});

Pebble.addEventListener('appmessage', function(e) {
  var p = e.payload;
  var id = p.REC_ID;
  if (p.MSG_TYPE === MSG_REC_META) {
    pending[id] = { meta: p, chunks: [] };
  } else if (p.MSG_TYPE === MSG_REC_CHUNK) {
    if (pending[id]) {
      pending[id].chunks[p.SEQ] = p.CHUNK;
    }
  } else if (p.MSG_TYPE === MSG_REC_DONE) {
    var rec = pending[id];
    delete pending[id];
    if (!rec) {
      console.log('done for unknown recording ' + id);
      return;
    }
    var all = [];
    for (var i = 0; i < rec.chunks.length; i++) {
      var chunk = rec.chunks[i];
      if (!chunk) {
        console.log('recording ' + id + ' missing chunk ' + i + ', dropping');
        return;
      }
      // chunk may be a plain array or a typed array depending on the runtime
      for (var j = 0; j < chunk.length; j++) {
        all.push(chunk[j]);
      }
    }
    upload(rec.meta, p.ACTUAL, all);
  }
});
