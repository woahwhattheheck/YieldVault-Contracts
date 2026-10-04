"""Execute the original contract, repair invocation budget isolation, retain source only."""
from pathlib import Path
import hashlib
import json
import os
import re
import subprocess
import sys
import tarfile
from datetime import datetime, timezone

BASE = '096a5dc4416a6faa57fdd80f60952eb7dcd6e11e'
CANDIDATE = 'candidate/yvc85-ttl-invocations-280b-20261004'
root = Path(sys.argv[1]).resolve()
out = Path(sys.argv[2]).resolve()
out.mkdir(parents=True, exist_ok=True)

def git(*args):
    return subprocess.check_output(['git', *args], cwd=root, text=True).strip()

def replace(path, before, after, count=1):
    file = root / path
    text = file.read_text()
    if text.count(before) != count:
        raise RuntimeError(f'{path}: expected {count} occurrences of {before!r}')
    file.write_text(text.replace(before, after))

def execute(name, command):
    with (out / (name + '.log')).open('w') as log:
        result = subprocess.run(command, cwd=root, stdout=log, stderr=subprocess.STDOUT, text=True)
    text = (out / (name + '.log')).read_text()
    print(text, flush=True)
    return result.returncode, text

assert git('rev-parse', 'HEAD') == BASE
assert git('status', '--porcelain') == ''
expected = {
    'src/lib.rs': '35870469416df3af91dc55b0d232afd06cb24e81',
    'src/storage.rs': '01e0f51da69172c759e230d76cd2df82e2c47521',
    'src/test.rs': '250e90dd5a414fde1afba92007a0f9c3dfb3bcef',
    'src/types.rs': '20ad84198f5082873df7d4ac6cd3f8a981cc0ea3',
    'README.md': 'a4127566ad3281b15fc3cddc9d4deab302d68682',
    'docs/ttl-and-rent.md': 'b8692700cea3ffafcfc38ff58c6d94e024b067fb',
}
for path, sha in expected.items():
    assert git('hash-object', path) == sha, path
versions = {name: subprocess.check_output([name, '--version'], cwd=root, text=True).strip()
            for name in ('rustc', 'cargo')}
regression = r'''

#[test]
fn test_ttl_separate_invocations_cannot_exhaust_other_users() {
    use crate::types::DataKey;
    use soroban_sdk::testutils::storage::Persistent as _;

    let t = VaultTest::setup();
    let cap = crate::storage::MAX_TTL_BUMPS_PER_INVOCATION;
    let mut users = std::vec::Vec::new();
    for i in 0..(cap + 4) {
        let user = Address::generate(&t.env);
        t.mint(&user, 100);
        assert_eq!(t.vault.deposit(&user, &100u128), 100);
        let (ttl, used) = t.env.as_contract(&t.vault.address, || {
            (t.env.storage().persistent().get_ttl(&DataKey::Balance(user.clone())),
             crate::storage::ttl_bump_count(&t.env))
        });
        assert!(ttl >= crate::storage::PERSISTENT_LIFETIME_THRESHOLD,
                "renewal budget leaked across invocations: depositor {i}, ttl {ttl}, used {used}");
        assert!(used <= cap);
        users.push(user);
    }
    assert_eq!(t.vault.total_shares(), u128::from(cap + 4) * 100);
    assert_eq!(t.vault.total_assets(), u128::from(cap + 4) * 100);

    advance_ledger(&t.env, crate::storage::DAY_IN_LEDGERS + 10);
    for (i, user) in users.iter().enumerate() {
        assert_eq!(t.vault.balance_of(user), 100);
        let (ttl, used) = t.env.as_contract(&t.vault.address, || {
            (t.env.storage().persistent().get_ttl(&DataKey::Balance(user.clone())),
             crate::storage::ttl_bump_count(&t.env))
        });
        assert!(ttl >= crate::storage::PERSISTENT_LIFETIME_THRESHOLD,
                "renewal budget leaked across invocations: reader {i}, ttl {ttl}, used {used}");
        assert!(used <= cap);
    }
    // Pure aggregate and conversion views must not reset or consume a budget.
    let used = t.env.as_contract(&t.vault.address, || crate::storage::ttl_bump_count(&t.env));
    assert_eq!(t.vault.total_shares(), u128::from(cap + 4) * 100);
    assert_eq!(t.vault.preview_deposit(&100u128), 100);
    assert_eq!(t.env.as_contract(&t.vault.address, || crate::storage::ttl_bump_count(&t.env)), used);
}
'''
with (root / 'src/test.rs').open('a') as f:
    f.write(regression)
baseline_command = ['cargo', 'test', '--locked', '--lib', 'test_ttl_separate_invocations_cannot_exhaust_other_users', '--', '--exact', 'test::test_ttl_separate_invocations_cannot_exhaust_other_users', '--nocapture']
# A single fully-qualified filter avoids accidentally accepting zero selected cases.
baseline_command = ['cargo', 'test', '--locked', '--lib', 'test::test_ttl_separate_invocations_cannot_exhaust_other_users', '--', '--exact', '--nocapture']
before_code, before = execute('before', baseline_command)
assert before_code != 0 and 'renewal budget leaked across invocations' in before
assert '1 failed' in before and 'running 1 test' in before

file = root / 'src/lib.rs'
text = file.read_text()
entrypoints = ('initialize', 'set_admin', 'balance_of', 'set_paused', 'set_min_deposit',
               'max_withdraw', 'share_percentage', 'max_redeem', 'deposit', 'withdraw',
               'accrue_yield', 'set_expected_wasm_hash', 'upgrade')
for name in entrypoints:
    pattern = r'(    pub fn ' + name + r'\([^\n]+\{\n)'
    text, count = re.subn(pattern, r'\1        storage::begin_invocation(&env);\n', text)
    assert count == 1, name
file.write_text(text)
helper = '''/// Start the budget for one public invocation without discarding ledger dedup.
///
/// Every entrypoint that can renew storage calls this before its first access.
/// A previous caller must not consume this invocation's extension allowance.
/// Pure views do not call this helper; an unused budget needs no storage write.
pub fn begin_invocation(env: &Env) {
    if ttl_bump_count(env) != 0 {
        env.storage().temporary().remove(&DataKey::TtlBumpCount);
    }
}

'''
replace('src/storage.rs', '/// Extend the time-to-live of the instance storage so the contract stays live.\n', helper + '/// Extend the time-to-live of the instance storage so the contract stays live.\n')
replace('src/storage.rs', '//! storage holds only per-ledger TTL bump dedup flags and the bump budget\n//! counter (see below).', '//! storage holds only per-ledger TTL bump dedup flags and the invocation bump\n//! counter (see below).')
replace('src/storage.rs', '//! - Per-ledger cap:', '//! - Per-invocation cap:')
replace('src/storage.rs', '//! Dedup and budget scratch keys are scoped by **ledger sequence**, so a bump', '//! Dedup flags are scoped by **ledger sequence**, so a bump')
replace('src/storage.rs', '//! because TTL is measured in ledgers.\n', '//! because TTL is measured in ledgers. Each TTL-touching public entrypoint starts\n//! a fresh budget through [`begin_invocation`], so unrelated callers cannot\n//! consume each other\'s allowance.\n')
replace('src/storage.rs', '/// Hard cap on `extend_ttl` calls issued by this contract for a single ledger\n/// sequence (instance + all persistent keys combined).', '/// Hard cap on `extend_ttl` calls issued by this contract for one invocation\n/// (instance + all persistent keys combined).')
replace('src/storage.rs', 'per-ledger bump budget', 'per-invocation bump budget', count=2)
replace('src/storage.rs', '/// Number of TTL bumps performed so far for the current ledger sequence.', '/// Number of TTL bumps in the latest TTL-touching invocation (zero in a new ledger).')
replace('src/types.rs', '/// within a single ledger sequence (see `storage` module docs).', '/// with ledger-scoped dedup and an invocation-scoped budget (see `storage`).')
replace('src/types.rs', '/// Temporary: stores the ledger sequence and its shared TTL bump count.', '/// Temporary: stores the ledger sequence and current invocation TTL bump count.')
replace('README.md', 'access share a budget of eight host calls per ledger sequence, across the', 'access share a budget of eight host calls per invocation, across the')
replace('README.md', 'and ledger-window reset.', 'and invocation reset. Earlier callers in the same ledger cannot exhaust a\nlater caller\'s allowance; key-level dedup still lasts for that ledger.')
replace('docs/ttl-and-rent.md', '| `TtlInstanceBumped`, `TtlBalanceBumped(user)`, `TtlBumpCount` | Temporary | Per-ledger scratch | Dedup/budget only; values are scoped by ledger sequence. |', '| `TtlInstanceBumped`, `TtlBalanceBumped(user)` | Temporary | Per-ledger scratch | Key-level dedup; values are scoped by ledger sequence. |\n| `TtlBumpCount` | Temporary | Invocation scratch | Reset at each TTL-touching public entrypoint; sequence prevents reuse across ledgers. |')
replace('docs/ttl-and-rent.md', '- Hard cap: `MAX_TTL_BUMPS_PER_INVOCATION` (8) bumps per ledger sequence.', '- Hard cap: `MAX_TTL_BUMPS_PER_INVOCATION` (8) bumps per invocation.')
replace('docs/ttl-and-rent.md', 'Dedup/budget scratch is keyed by ledger sequence, so activity in ledger *N*', 'Dedup scratch is keyed by ledger sequence, so activity in ledger *N*')
replace('docs/ttl-and-rent.md', 'bumps are redundant because TTL is measured in ledgers.', 'bumps are redundant because TTL is measured in ledgers. The extension counter,\nhowever, resets at the start of each public invocation that can touch TTL. This\nprevents seven earlier depositors from consuming the renewal allowance of\nlater depositors in the same ledger. Pure configuration, aggregate and\nconversion views do not reset the counter. No business storage keys, entrypoint\nsignatures, authorization rules or accounting calculations change.')
after_command = ['cargo', 'test', '--locked', '--lib']
after_code, after = execute('after', after_command)
assert after_code == 0 and 'test::test_ttl_separate_invocations_cannot_exhaust_other_users ... ok' in after
assert '0 failed' in after
summary = re.findall(r'test result: ok\. [^\n]+', after)[-1]
receipt = {'base': BASE, 'versions': versions, 'before_command': baseline_command,
           'before_exit': before_code, 'after_command': after_command, 'after_exit': after_code,
           'after_summary': summary, 'entrypoints': list(entrypoints),
           'production_blobs': {p: git('hash-object', p) for p in expected},
           'observed_at': datetime.now(timezone.utc).isoformat()}
(out / 'receipt.json').write_text(json.dumps(receipt, indent=2) + '\n')
with (root / 'docs/ttl-and-rent.md').open('a') as f:
    f.write('\n## Invocation-isolation regression\n\n')
    f.write('The unchanged contract at `' + BASE + '` failed the new real-Soroban\n')
    f.write('`test_ttl_separate_invocations_cannot_exhaust_other_users`: later depositors\n')
    f.write('received an unextended balance entry after earlier callers consumed the\nshared allowance. With invocation initialization, twelve distinct depositors\n')
    f.write('and twelve later renewal reads receive their configured TTL in one ledger.\n')
    f.write('Aggregate balances and pure-view budget behavior are also checked.\n\n')
    f.write('Execution: `' + versions['rustc'] + '`, `' + versions['cargo'] + '`.\n')
    f.write('`cargo test --locked --lib`: **' + summary + '**\n\n')
    f.write('Existing dedup, per-invocation exhaustion and expiration cases are retained\nunchanged. This is native contract execution, not a live-chain deployment,\nperformance benchmark, sponsor acceptance or payment receipt.\n')
assert set(git('diff', '--name-only').splitlines()) == set(expected)
subprocess.run(['git', 'add', *expected], cwd=root, check=True)
subprocess.run(['git', '-c', 'user.name=woahwhattheheck', '-c', 'user.email=293286387+woahwhattheheck@users.noreply.github.com', 'commit', '-m', 'fix(storage): isolate TTL budgets between invocations [skip ci]'], cwd=root, check=True)
assert git('rev-parse', 'HEAD^') == BASE
candidate = git('rev-parse', 'HEAD')
(out / 'candidate.txt').write_text(candidate + '\n' + git('rev-parse', 'HEAD^{tree}') + '\n')
(out / 'source.patch').write_text(git('diff', BASE, 'HEAD') + '\n')
with tarfile.open(out / 'source.tar.gz', 'w:gz') as tar:
    for path in expected:
        tar.add(root / path, arcname=path)
subprocess.run(['git', 'push', 'origin', 'HEAD:refs/heads/' + CANDIDATE], cwd=root, check=True)
print('CANDIDATE', candidate, 'TREE', git('rev-parse', 'HEAD^{tree}'), flush=True)
