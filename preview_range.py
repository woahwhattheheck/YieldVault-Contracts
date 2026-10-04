"""Two disjoint existing-PR follow-throughs; never advances either original branch."""
from pathlib import Path
import hashlib
import json
import re
import subprocess
import sys
import tarfile

mode, root_arg, out_arg = sys.argv[1:]
root, out = Path(root_arg).resolve(), Path(out_arg).resolve()
out.mkdir(parents=True, exist_ok=True)

def git(*args):
    return subprocess.check_output(['git', *args], cwd=root, text=True).strip()

def run(name, args):
    with (out / (name + '.log')).open('w') as stream:
        proc = subprocess.run(args, cwd=root, text=True, stdout=stream, stderr=subprocess.STDOUT)
    text = (out / (name + '.log')).read_text()
    print(text, flush=True)
    return proc.returncode, text

def commit_and_retain(base, files, branch, message):
    changed = set(git('diff', '--name-only').splitlines())
    assert set(files).issubset(changed), changed
    generated = changed - set(files)
    assert all(p.startswith('test_snapshots/') and p.endswith('.json') for p in generated), generated
    (out / 'generated-not-published.json').write_text(json.dumps(sorted(generated), indent=2))
    subprocess.run(['git', 'add', *files], cwd=root, check=True)
    subprocess.run(['git', '-c', 'user.name=woahwhattheheck', '-c', 'user.email=293286387+woahwhattheheck@users.noreply.github.com', 'commit', '-m', message], cwd=root, check=True)
    assert git('rev-parse', 'HEAD^') == base
    receipt = {'base': base, 'candidate': git('rev-parse', 'HEAD'), 'tree': git('rev-parse', 'HEAD^{tree}'),
               'blobs': {p: git('hash-object', p) for p in files}}
    (out / 'source.json').write_text(json.dumps(receipt, indent=2) + '\n')
    (out / 'source.patch').write_text(git('diff', base, 'HEAD') + '\n')
    with tarfile.open(out / 'source.tar.gz', 'w:gz') as archive:
        for path in files:
            archive.add(root / path, arcname=path)
    subprocess.run(['git', 'push', 'origin', 'HEAD:refs/heads/' + branch], cwd=root, check=True)
    print(json.dumps(receipt), flush=True)

assert git('status', '--porcelain') == ''
if mode == 'format':
    base = '477f418eac85d27be45358af7e2ffc6e514afca1'
    assert git('rev-parse', 'HEAD') == base
    before = (root / 'src/test.rs').read_text()
    code, _ = run('rustfmt', ['rustfmt', '--edition', '2021', 'src/test.rs'])
    assert code == 0
    after = (root / 'src/test.rs').read_text()
    # rustfmt inserts optional trailing commas, but no substantive source tokens.
    assert re.sub(r'[\s,]', '', before) == re.sub(r'[\s,]', '', after)
    assert git('diff', '--name-only') == 'src/test.rs'
    commit_and_retain(base, ['src/test.rs'], 'candidate/yvc85-formatted-280b-20261004',
                      'style: format the TTL invocation regression [skip ci]')
    raise SystemExit(0)
assert mode == 'repair'
base = '7bb2380b5c8c47b8a91f16329db737d698c41b06'
assert git('rev-parse', 'HEAD') == base
assert git('hash-object', 'src/lib.rs') == '710377bd5fe7c66749c20aeb7b6f8cc05b333214'
assert git('hash-object', 'src/test.rs') == '3103878da646e402e947ee9cde834d31e95acf95'
regression = r'''

#[test]
fn test_preview_token_range_deposit_rejects_unrepresentable_assets() {
    let t = VaultTest::setup();
    let user = Address::generate(&t.env);
    let amount = i128::MAX as u128 + 1;
    assert_eq!(t.vault.convert_to_shares(&amount), amount);
    assert_eq!(t.vault.try_preview_deposit(&amount), Err(Ok(crate::Error::MathOverflow)),
               "signed token amount unchecked in deposit preview");
    assert_eq!(t.vault.try_deposit(&user, &amount), Err(Ok(crate::Error::MathOverflow)));
    assert_eq!(t.vault.total_shares(), 0);
    assert_eq!(t.vault.total_assets(), 0);
    assert_eq!(t.vault.balance_of(&user), 0);
    assert_eq!(t.token.balance(&user), 0);
    assert_eq!(t.token.balance(&t.vault.address), 0);
}

#[test]
fn test_preview_token_range_withdraw_rejects_unrepresentable_assets() {
    let t = VaultTest::setup();
    let user = Address::generate(&t.env);
    let assets = i128::MAX as u128 + 1;
    t.env.as_contract(&t.vault.address, || {
        crate::storage::set_total_shares(&t.env, 1);
        crate::storage::set_total_assets(&t.env, assets);
        crate::storage::set_balance(&t.env, &user, 1);
    });
    assert_eq!(t.vault.convert_to_assets(&1), assets);
    assert_eq!(t.vault.try_preview_withdraw(&1), Err(Ok(crate::Error::MathOverflow)),
               "signed token amount unchecked in withdrawal preview");
    assert_eq!(t.vault.try_withdraw(&user, &1), Err(Ok(crate::Error::MathOverflow)));
    assert_eq!(t.vault.total_shares(), 1);
    assert_eq!(t.vault.total_assets(), assets);
    assert_eq!(t.vault.balance_of(&user), 1);
    assert_eq!(t.token.balance(&user), 0);
    assert_eq!(t.token.balance(&t.vault.address), 0);
}

#[test]
fn test_preview_token_range_maximum_positive_amount_remains_valid() {
    let amount = i128::MAX as u128;
    let deposit = VaultTest::setup();
    let user = Address::generate(&deposit.env);
    deposit.mint(&user, i128::MAX);
    assert_eq!(deposit.vault.preview_deposit(&amount), amount);
    assert_eq!(deposit.vault.deposit(&user, &amount), amount);
    assert_eq!(deposit.token.balance(&deposit.vault.address), i128::MAX);

    let withdrawal = VaultTest::setup();
    let holder = Address::generate(&withdrawal.env);
    withdrawal.mint(&withdrawal.vault.address, i128::MAX);
    withdrawal.env.as_contract(&withdrawal.vault.address, || {
        crate::storage::set_total_shares(&withdrawal.env, 1);
        crate::storage::set_total_assets(&withdrawal.env, amount);
        crate::storage::set_balance(&withdrawal.env, &holder, 1);
    });
    assert_eq!(withdrawal.vault.preview_withdraw(&1), amount);
    assert_eq!(withdrawal.vault.withdraw(&holder, &1), amount);
    assert_eq!(withdrawal.token.balance(&holder), i128::MAX);
    assert_eq!(withdrawal.vault.total_shares(), 0);
    assert_eq!(withdrawal.vault.total_assets(), 0);
}
'''
with (root / 'src/test.rs').open('a') as file:
    file.write(regression)
command = ['cargo', 'test', '--locked', '--lib', 'test_preview_token_range_', '--', '--nocapture']
code, before = run('before', command)
assert code != 0 and 'running 3 tests' in before
assert 'signed token amount unchecked in deposit preview' in before
assert 'signed token amount unchecked in withdrawal preview' in before
assert '1 passed; 2 failed' in before

path = root / 'src/lib.rs'
text = path.read_text()
def replace(old, new):
    global text
    assert text.count(old) == 1, old
    text = text.replace(old, new)

replace('    if assets < storage::get_min_deposit(env) {\n        return Err(Error::BelowMinimumDeposit);\n    }\n',
        '    if assets < storage::get_min_deposit(env) {\n        return Err(Error::BelowMinimumDeposit);\n    }\n    token_amount(assets)?;\n')
replace('    if assets == 0 {\n        return Err(Error::ZeroAmount);\n    }\n    Ok(assets)\n}',
        '    if assets == 0 {\n        return Err(Error::ZeroAmount);\n    }\n    token_amount(assets)?;\n    Ok(assets)\n}')
replace('client.transfer(&from, &env.current_contract_address(), &(amount as i128));',
        'client.transfer(&from, &env.current_contract_address(), &token_amount(amount)?);')
replace('client.transfer(&env.current_contract_address(), &from, &(assets as i128));',
        'client.transfer(&env.current_contract_address(), &from, &token_amount(assets)?);')
text += '\n/// Token transfers use signed amounts; previews must reject the same range.\nfn token_amount(assets: u128) -> Result<i128, Error> {\n    i128::try_from(assets).map_err(|_| Error::MathOverflow)\n}\n'
path.write_text(text)
# Keep formatter changes scoped to the two claimed Rust files (no module recursion).
code, _ = run('rustfmt', ['rustfmt', '--edition', '2021', '--config', 'skip_children=true', 'src/lib.rs', 'src/test.rs'])
assert code == 0
command_after = ['cargo', 'test', '--locked', '--lib']
code, after = run('after', command_after)
assert code == 0
for name in ('deposit_rejects_unrepresentable_assets', 'withdraw_rejects_unrepresentable_assets', 'maximum_positive_amount_remains_valid'):
    assert 'test::test_preview_token_range_' + name + ' ... ok' in after
summary = re.findall(r'test result: ok\. [^\n]+', after)[-1]
versions = {name: subprocess.check_output([name, '--version'], cwd=root, text=True).strip()
            for name in ('rustc', 'cargo')}
receipt = {'base': base, 'versions': versions, 'before_command': command, 'before_exit': 101,
           'before_result': '1 passed; 2 failed', 'after_command': command_after,
           'after_exit': code, 'after_result': summary,
           'boundary': 'Actual native Soroban and Stellar Asset Contract with synthetic records and mocked user authorization; no live chain or sponsor acceptance.'}
(out / 'execution.json').write_text(json.dumps(receipt, indent=2) + '\n')
with (root / 'README.md').open('a') as file:
    file.write('\n### Signed token amount boundary\n\n')
    file.write('Checked deposit and withdrawal previews reject an underlying asset amount\nabove `i128::MAX` with the existing `MathOverflow` error. The same checked\nconversion is used by the token transfers, preventing a positive `u128` from\nwrapping into a negative token amount. Zero/minimum/paused/dust precedence\nand pure `convert_*` arithmetic remain unchanged. The maximum positive signed\namount remains accepted. Authorization, user balance and actual token liquidity\nremain mutation-only conditions; a preview is not a guarantee of settlement.\n\n')
    file.write('On base `' + base + '`, the real-contract range regression\nrecorded **1 passed / 2 failed**. The correction preserves the valid maximum\nand rejects the first unrepresentable value in both previews and mutations,\nwith stored balances unchanged on rejection.\n\n')
    file.write('Native `' + ' '.join(command_after) + '`: **' + summary + '**\n')
    file.write('Toolchain: `' + versions['rustc'] + '`, `' + versions['cargo'] + '`.\n')
    file.write('This uses the actual Soroban runtime and Stellar Asset Contract with\nsynthetic records and mocked user authorization, not a live-chain deployment\nor a payment/acceptance receipt. Existing ignored cases remain unchanged.\n')
commit_and_retain(base, ['src/lib.rs', 'src/test.rs', 'README.md'],
                  'candidate/yvc81-signed-range-280b-20261004',
                  'fix(contract): align preview and token transfer amount bounds [skip ci]')
