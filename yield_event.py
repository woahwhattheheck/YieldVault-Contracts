"""Source-pinned native event regression and a documentation-only correction."""
from pathlib import Path
import json
import re
import subprocess
import sys
import tarfile
mode, source, output = sys.argv[1:]
root, out = Path(source).resolve(), Path(output).resolve()
out.mkdir(parents=True, exist_ok=True)
def git(*args):
    return subprocess.check_output(['git', *args], cwd=root, text=True).strip()
def execute(name, args):
    with (out / (name + '.log')).open('w') as log:
        p = subprocess.run(args, cwd=root, stdout=log, stderr=subprocess.STDOUT, text=True)
    text = (out / (name + '.log')).read_text()
    print(text, flush=True)
    return p.returncode, text
def publish(base, files, branch, message):
    actual = set(git('diff', '--name-only').splitlines())
    assert set(files).issubset(actual), actual
    extras = actual - set(files)
    assert all(p.startswith('test_snapshots/') and p.endswith('.json') for p in extras), extras
    (out / 'snapshots-not-published.json').write_text(json.dumps(sorted(extras)))
    subprocess.run(['git', 'add', *files], cwd=root, check=True)
    subprocess.run(['git', '-c', 'user.name=woahwhattheheck', '-c', 'user.email=293286387+woahwhattheheck@users.noreply.github.com', 'commit', '-m', message], cwd=root, check=True)
    assert git('rev-parse', 'HEAD^') == base
    receipt = {'base': base, 'candidate': git('rev-parse', 'HEAD'), 'tree': git('rev-parse', 'HEAD^{tree}'),
               'blobs': {p: git('hash-object', p) for p in files}}
    (out / 'source.json').write_text(json.dumps(receipt, indent=2) + '\n')
    (out / 'source.patch').write_text(git('diff', base, 'HEAD') + '\n')
    with tarfile.open(out / 'source.tar.gz', 'w:gz') as archive:
        for p in files:
            archive.add(root / p, arcname=p)
    subprocess.run(['git', 'push', 'origin', 'HEAD:refs/heads/' + branch], cwd=root, check=True)
    print(json.dumps(receipt), flush=True)
assert git('status', '--porcelain') == ''
if mode == 'guide':
    base = 'fc2d4d5a5418ee8cc655fcbc7691cfc54d5800e0'
    assert git('rev-parse', 'HEAD') == base
    path = root / 'README.md'
    text = path.read_text()
    old = '| `is_initialized()` | Whether the vault has been set up. |\n'
    assert text.count(old) == 1 and '| `is_paused()` |' not in text
    path.write_text(text.replace(old, old + '| `is_paused()` | Whether deposits are currently paused. |\n'))
    publish(base, ['README.md'], 'candidate/yvc81-guide-restored-280b-20261004',
            'docs: retain the existing pause getter in the entrypoint inventory [skip ci]')
    raise SystemExit(0)
assert mode == 'event'
base = '3ed5f5a13be1320120597bc07490b4113e7eb1a8'
assert git('rev-parse', 'HEAD') == base
assert git('hash-object', 'src/lib.rs') == '3de2a8ab1aa6cee0d05b4145aafa1a732fb7cfb8'
assert git('hash-object', 'src/events.rs') == 'a478d99e81f8a6ff034979505cc090b13f0a78a9'
regression = r'''

#[test]
fn test_yield_event_records_actual_saturated_credit() {
    use soroban_sdk::{Symbol, TryFromVal, Val, Vec};
    // Normal credit, partially saturated credit, and a successful zero-delta call.
    for (before, requested, credited) in [(10u128, 7u128, 7u128), (u128::MAX - 2, 7, 2), (u128::MAX, 7, 0)] {
        let t = VaultTest::setup();
        t.env.as_contract(&t.vault.address, || {
            crate::storage::set_total_assets(&t.env, before);
            crate::storage::set_total_shares(&t.env, 7);
        });
        let old_events = t.env.events().all().len();
        t.vault.accrue_yield(&requested);
        let events = t.env.events().all();
        assert_eq!(events.len(), old_events + 1);
        let (contract, topics, payload) = events.last().unwrap();
        assert_eq!(contract, t.vault.address);
        assert_eq!(topics.len(), 3);
        assert_eq!(Symbol::try_from_val(&t.env, &topics.get(0).unwrap()).unwrap(), Symbol::new(&t.env, "yield"));
        assert_eq!(u32::try_from_val(&t.env, &topics.get(1).unwrap()).unwrap(), crate::types::EVENT_SCHEMA_VERSION);
        assert_eq!(Address::try_from_val(&t.env, &topics.get(2).unwrap()).unwrap(), t.admin);
        let data = Vec::<Val>::try_from_val(&t.env, &payload).unwrap();
        assert_eq!(data.len(), 7);
        assert_eq!(Address::try_from_val(&t.env, &data.get(0).unwrap()).unwrap(), t.token.address);
        assert_eq!(u128::try_from_val(&t.env, &data.get(1).unwrap()).unwrap(), credited,
                   "yield event reports request instead of actual credit: before={before}, requested={requested}");
        assert_eq!(u128::try_from_val(&t.env, &data.get(2).unwrap()).unwrap(), 0);
        assert_eq!(u128::try_from_val(&t.env, &data.get(3).unwrap()).unwrap(), before.saturating_add(requested));
        assert_eq!(u128::try_from_val(&t.env, &data.get(4).unwrap()).unwrap(), 7);
        assert_eq!(u32::try_from_val(&t.env, &data.get(5).unwrap()).unwrap(), t.env.ledger().sequence());
        assert_eq!(Symbol::try_from_val(&t.env, &data.get(6).unwrap()).unwrap(), Symbol::new(&t.env, "ok"));
        assert_eq!(t.vault.total_assets() - before, credited);
        assert_eq!(t.vault.total_shares(), 7);
    }
}
'''
with (root / 'src/test.rs').open('a') as file:
    file.write(regression)
cmd = ['cargo', 'test', '--locked', '--lib', 'test::test_yield_event_records_actual_saturated_credit', '--', '--exact', '--nocapture']
code, before = execute('before', cmd)
assert code != 0 and 'running 1 test' in before and '1 failed' in before
assert 'yield event reports request instead of actual credit' in before
path = root / 'src/lib.rs'
text = path.read_text()
old = '        let total_assets = storage::get_total_assets(&env).saturating_add(amount);\n'
new = '        let previous_assets = storage::get_total_assets(&env);\n        let total_assets = previous_assets.saturating_add(amount);\n        let credited_assets = total_assets - previous_assets;\n'
assert text.count(old) == 1
text = text.replace(old, new)
old = '            &token_address,\n            amount,\n            total_assets,\n            total_shares,\n'
new = '            &token_address,\n            credited_assets,\n            total_assets,\n            total_shares,\n'
assert text.count(old) == 1
path.write_text(text.replace(old, new))
# Restrict formatter writes to claimed Rust files; keep shared event publisher exact.
code, _ = execute('format', ['rustfmt', '--edition', '2021', '--config', 'skip_children=true', 'src/lib.rs', 'src/test.rs'])
assert code == 0
assert git('hash-object', 'src/events.rs') == 'a478d99e81f8a6ff034979505cc090b13f0a78a9'
cmd_after = ['cargo', 'test', '--locked', '--lib']
code, after = execute('after', cmd_after)
assert code == 0 and 'test::test_yield_event_records_actual_saturated_credit ... ok' in after
summary = re.findall(r'test result: ok\. [^\n]+', after)[-1]
versions = {name: subprocess.check_output([name, '--version'], cwd=root, text=True).strip() for name in ('rustc', 'cargo')}
(out / 'execution.json').write_text(json.dumps({'base': base, 'before_command': cmd, 'before_exit': 101,
  'before_result': '1 failed', 'after_command': cmd_after, 'after_exit': code, 'after_result': summary,
  'versions': versions, 'scope': 'Real native Soroban contract and event decoding; synthetic records, mocked authorization; no live chain or award.'}, indent=2) + '\n')
with (root / 'docs/event-reference.md').open('a') as file:
    file.write('\n## Saturation and actual credited yield\n\n')
    file.write('The schema-v1 yield amount_assets field is the actual increase in stored\ntotal_assets, not the requested mock-yield amount. When saturating arithmetic\nreaches u128::MAX, a partially credited request emits only the credited delta;\na successful request at the cap emits a zero amount. The existing one-event\nbehavior, successful outcome, schema layout/version, actor, asset, correlation\nand share totals are unchanged. No accounting policy or migration is added.\n\n')
    file.write('At original source `' + base + '`, the new actual-contract\nregression failed: a request for7 at u128::MAX-2 emitted7 even though only2\nwere credited. The corrected case additionally covers ordinary credit and\na zero-delta call at the cap, decoding all topic/payload fields.\n\n')
    file.write('Native `cargo test --locked --lib`: **' + summary + '**\n')
    file.write('Toolchain: `' + versions['rustc'] + '`, `' + versions['cargo'] + '`.\n')
    file.write('Existing emission, fixture parser, compatibility and lifecycle tests are\npreserved. This uses real Soroban with synthetic records and mocked\nauthorization; it is not a live-chain, indexer deployment or payment receipt.\n')
publish(base, ['src/lib.rs', 'src/test.rs', 'docs/event-reference.md'],
        'candidate/yvc88-yield-event-280b-20261004',
        'fix(events): publish the actual credited yield delta [skip ci]')
