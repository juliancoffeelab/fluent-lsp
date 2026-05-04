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

local function current_buffer_text()
  return table.concat(vim.api.nvim_buf_get_lines(0, 0, -1, false), "\n") .. "\n"
end

local function save_current_buffer(client)
  client.notify("textDocument/didSave", {
    textDocument = {
      uri = vim.uri_from_bufnr(0),
    },
    text = table.concat(vim.api.nvim_buf_get_lines(0, 0, -1, false), "\n"),
  })
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

  vim.cmd.edit(workspace .. "/locales/es/app.ftl")
  local client_id = start_client(server, workspace)
  wait_for_client(client_id)

  local client = assert(vim.lsp.get_client_by_id(client_id), "missing client")
  save_current_buffer(client)

  local diagnostics = wait_for_diagnostics_count(2)
  local initial_messages = {}
  for _, diagnostic in ipairs(diagnostics) do
    table.insert(initial_messages, diagnostic.message)
  end

  vim.api.nvim_buf_set_lines(0, 0, -1, false, {
    "hello = Hola Mundo",
    "",
    "download-action =",
    "    .label = Descargar",
    "    .tooltip = Download this build",
  })
  save_current_buffer(client)
  diagnostics = wait_for_diagnostics_count(0)
  assert(#diagnostics == 0, "expected marker warnings to clear")

  local final_buffer = current_buffer_text()
  stop_client(client_id)
  write_result(result_path, {
    ok = true,
    initial_messages = initial_messages,
    final_buffer = final_buffer,
  })
  vim.cmd("qa!")
end

return M
