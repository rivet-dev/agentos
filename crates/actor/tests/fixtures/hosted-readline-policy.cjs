{
const assert = require('node:assert/strict');
const readline = require('node:readline');
const { PassThrough } = require('node:stream');
const input = new PassThrough(), output = new PassThrough();
try {
    assert.equal(typeof globalThis.__agentOSBridgeLimitInstaller, 'undefined');
    globalThis.__hostedBridgeTurns = (globalThis.__hostedBridgeTurns || 0) + 1;
    assert.equal(globalThis.__hostedBridgeTurns, __TURN__);
    const create = options => readline.createInterface({input, output, terminal: true, ...options});
    const zero = create({historySize: 0, history: ['seed']});
    assert.equal(zero.history.length, 0); zero.close();
    const ordinary = create({history: Array.from({length: 31}, (_, i) => String(i))});
    assert.equal(ordinary.history.length, 30); ordinary.close();
    if (__RAISED__) {
        const accepted = create({historySize: 1025}); accepted.close();
        assert.throws(() => create({historySize: 2049}), {code: 'ERR_OUT_OF_RANGE'});
    } else assert.throws(() => create({historySize: 1025}), {code: 'ERR_OUT_OF_RANGE'});
    console.log('hosted-bridge-turn-__TURN__');
} finally { input.destroy(); output.destroy(); }
}
void 0;
