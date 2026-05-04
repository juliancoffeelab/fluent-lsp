local M = {}

local function write_result(result_path, payload)
  vim.fn.writefile({ vim.json.encode(payload) }, result_path)
end

local function start_client(server, workspace)
  local client_id = vim.lsp.start({
    name = "fluent-lsp",
    cmd = { server },
    root_dir = workspace,
  })
  assert(client_id, "failed to start fluent-lsp")
  return client_id
end

local function wait_for_client(client_id)
  local ready = vim.wait(3000, function()
    for _, client in ipairs(vim.lsp.get_clients({ bufnr = 0 })) do
      if client.id == client_id then
        return true
      end
    end
    return false
  end, 50)
  assert(ready, "client not attached")
end

local function diagnostics_for_current_buffer()
  return vim.diagnostic.get(0)
end

local function wait_for_diagnostics_count(expected)
  local ready = vim.wait(3000, function()
    return #diagnostics_for_current_buffer() == expected
  end, 50)
  assert(ready, "unexpected diagnostic count: " .. #diagnostics_for_current_buffer())
  return diagnostics_for_current_buffer()
end

local function stop_client(client_id)
  vim.lsp.stop_client(client_id, true)
  vim.wait(3000, function()
    for _, client in ipairs(vim.lsp.get_clients()) do
      if client.id == client_id then
        return false
      end
    end
    return true
  end, 50)
end

function M.run()
  local workspace = assert(vim.env.FLUENT_LSP_WORKSPACE ~= "" and vim.env.FLUENT_LSP_WORKSPACE)
  local server = assert(vim.env.FLUENT_LSP_BIN ~= "" and vim.env.FLUENT_LSP_BIN)
  local result_path = assert(vim.env.FLUENT_LSP_RESULT ~= "" and vim.env.FLUENT_LSP_RESULT)
  local broken_path = workspace .. "/locales/en/broken.ftl"

  vim.fn.writefile({
    "welcome-title = Welcome",
    "",
    "g@Rb@ge = broken",
  }, broken_path)

  vim.cmd.edit(broken_path)
  local client_id = start_client(server, workspace)
  wait_for_client(client_id)

  local diagnostics = diagnostics_for_current_buffer()
  assert(#diagnostics == 0, "parse diagnostics should wait for save")

  vim.cmd.write()

  diagnostics = wait_for_diagnostics_count(1)
  assert(
    diagnostics[1].message == 'Fluent syntax error: Expected a token starting with "="',
    "unexpected parse diagnostic message"
  )
  assert(
    diagnostics[1].severity == vim.diagnostic.severity.ERROR,
    "parse diagnostics should use error severity"
  )
  assert(diagnostics[1].lnum == 2, "parse diagnostic should point at the invalid line")
  assert(diagnostics[1].col == 1, "parse diagnostic should point at the invalid token")
  assert(diagnostics[1].end_lnum == 2, "parse diagnostic should stay on the invalid line")
  assert(diagnostics[1].end_col == 2, "parse diagnostic should highlight the invalid token")

  vim.api.nvim_buf_set_lines(0, 2, 3, false, { "garbage = broken" })
  vim.cmd.write()

  diagnostics = wait_for_diagnostics_count(0)
  assert(#diagnostics == 0, "parse diagnostics should clear after a valid save")

  stop_client(client_id)
  write_result(result_path, {
    ok = true,
    final_buffer = table.concat(vim.api.nvim_buf_get_lines(0, 0, -1, false), "\n") .. "\n",
  })
  vim.cmd("qa!")
end

return M
