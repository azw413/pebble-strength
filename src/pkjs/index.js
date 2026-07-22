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
