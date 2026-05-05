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

local function wait_for_client(client_id, capability)
  local ready = vim.wait(3000, function()
    for _, client in ipairs(vim.lsp.get_clients({ bufnr = 0 })) do
      if client.id == client_id and client.server_capabilities[capability] then
        return true
      end
    end
    return false
  end, 50)
  assert(ready, capability .. " not ready")
end

local function code_action_params()
  local cursor = vim.api.nvim_win_get_cursor(0)
  return {
    textDocument = { uri = vim.uri_from_bufnr(0) },
    range = {
      start = { line = cursor[1] - 1, character = cursor[2] },
      ["end"] = { line = cursor[1] - 1, character = cursor[2] },
    },
    context = {
      diagnostics = {},
    },
  }
end

local function request_code_actions(client_id)
  local responses = vim.lsp.buf_request_sync(0, "textDocument/codeAction", code_action_params(), 3000)
  local result = assert(responses[client_id] and responses[client_id].result, "missing code actions")
  assert(#result > 0, "expected at least one code action")
  return result
end

local function hover_params()
  local cursor = vim.api.nvim_win_get_cursor(0)
  return {
    textDocument = { uri = vim.uri_from_bufnr(0) },
    position = {
      line = cursor[1] - 1,
      character = cursor[2],
    },
  }
end

local function request_hover(client_id)
  local responses = vim.lsp.buf_request_sync(0, "textDocument/hover", hover_params(), 3000)
  local hover = assert(responses[client_id] and responses[client_id].result, "missing hover")
  return hover.contents.value
end

local function find_action(actions, title)
  for _, action in ipairs(actions) do
    if action.title == title then
      return action
    end
  end
  error("missing code action: " .. title)
end

local function current_buffer_text()
  return table.concat(vim.api.nvim_buf_get_lines(0, 0, -1, false), "\n") .. "\n"
end

local function flush_changes()
  vim.wait(150, function()
    return true
  end, 50)
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
  local source_path = workspace .. "/locales/es/app.ftl"

  vim.cmd.edit(source_path)
  local original_lines = vim.api.nvim_buf_get_lines(0, 0, -1, false)
  local client_id = start_client(server, workspace)
  wait_for_client(client_id, "codeActionProvider")
  wait_for_client(client_id, "hoverProvider")
  local client = assert(vim.lsp.get_client_by_id(client_id), "missing client")

  local found = vim.fn.searchpos('hello = { "" }', "n")
  vim.api.nvim_win_set_cursor(0, { found[1], found[2] - 1 })
  local actions = request_code_actions(client_id)
  local action = find_action(actions, "Copy `hello` from source")
  vim.lsp.util.apply_workspace_edit(action.edit, client.offset_encoding or "utf-16")
  local copied_key_buffer = current_buffer_text()

  vim.api.nvim_buf_set_lines(0, 0, -1, false, original_lines)
  flush_changes()

  found = vim.fn.searchpos("download-action =", "n")
  vim.api.nvim_win_set_cursor(0, { found[1], found[2] - 1 })
  actions = request_code_actions(client_id)
  action = find_action(actions, "Copy missing attributes for `download-action` from source")
  vim.lsp.util.apply_workspace_edit(action.edit, client.offset_encoding or "utf-16")
  local copied_attribute_buffer = current_buffer_text()
  found = vim.fn.searchpos(".tooltip = Download this build", "n")
  vim.api.nvim_win_set_cursor(0, { found[1], found[2] - 1 })
  local copied_attribute_hover = request_hover(client_id)

  stop_client(client_id)
  write_result(result_path, {
    ok = true,
    copied_key_buffer = copied_key_buffer,
    copied_attribute_buffer = copied_attribute_buffer,
    copied_attribute_hover = copied_attribute_hover,
  })
  vim.cmd("qa!")
end

return M
