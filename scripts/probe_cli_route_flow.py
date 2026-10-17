"""Real CLI starter for the original-page mixed Agent three-peer route fixture."""
import subprocess

def begin(c, name):
    identity = c.agent_catalogue()[1][0][0]
    expected = c.log.read_text().splitlines().count('workspace.cli_ssh_approval_requested') + 1
    output = (c.evidence / (c.prefix + '-' + name + '-client.txt')).open('wb')
    process = subprocess.Popen(['target/debug/pulse', 'ssh', c.host_id, '--identity', identity],
                               stdout=output, stderr=subprocess.STDOUT, start_new_session=True)
    c.cli_clients.append((process, output))
    c.wait(lambda: c.log.read_text().splitlines().count('workspace.cli_ssh_approval_requested') == expected, 'route_cli_approval_' + name)
    c.reveal('连接'); c.endpoint_screenshot('cli-' + name + '-target'); c.click('连接')
    return process

def finished(c, process, success):
    assert process.wait(timeout=5) == (0 if success else 1)
    if success:
        assert c.log.read_text().splitlines().count('workspace.cli_ssh_route_opened') == 1
