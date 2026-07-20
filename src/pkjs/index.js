// Strength — phone-side relay (SPEC.md §3).
// M2 scope: reassemble set recordings from the watch and POST them to the
// server as the labelled tuning corpus. Workout download sync arrives in M3.

// For the emulator this reaches the dev server directly. For a physical
// watch, set this to your machine's LAN address, e.g. 'http://192.168.1.20:8080'.
var SERVER = 'http://192.168.1.36:8090';

var MSG_REC_META = 0;
var MSG_REC_CHUNK = 1;
var MSG_REC_DONE = 2;

var pending = {};

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
  xhr.open('POST', SERVER + '/api/device/recordings');
  xhr.setRequestHeader('Content-Type', 'application/json');
  xhr.onload = function() {
    console.log('recording upload: ' + xhr.status + ' ' + xhr.responseText);
  };
  xhr.onerror = function() {
    console.log('recording upload failed (server unreachable at ' + SERVER + ')');
  };
  xhr.send(body);
}

Pebble.addEventListener('ready', function() {
  console.log('Strength pkjs ready, server: ' + SERVER);
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
