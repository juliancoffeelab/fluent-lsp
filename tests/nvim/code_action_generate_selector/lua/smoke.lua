local M = {}

local function write_result(result_path, payload)
  vim.fn.writefile({ vim.json.encode(payload) }, result_path)
end

local function start_client(server, workspace)
  local client_id = vim.lsp.start({
    name = "fluent-lsp",
    cmd = { server },
    root_dir = workspace,
    capabilities = {
      workspace = {
        workspaceEdit = {
          documentChanges = true,
          snippetEditSupport = true,
        },
      },
    },
  })
  assert(client_id, "failed to start fluent-lsp")
  return client_id
end

local function wait_for_client(client_id, capability)
  local ready = vim.wait(5000, function()
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
  local responses = vim.lsp.buf_request_sync(0, "textDocument/codeAction", code_action_params(), 5000)
  local result = assert(responses[client_id] and responses[client_id].result, "missing code actions")
  assert(#result > 0, "expected at least one code action")
  return result
end

local function request_code_actions_allow_empty(client_id)
  local responses = vim.lsp.buf_request_sync(0, "textDocument/codeAction", code_action_params(), 5000)
  local result = responses[client_id] and responses[client_id].result
  if result == nil then
    return {}
  end
  return result
end

local function find_action(actions, title)
  for _, action in ipairs(actions) do
    if action.title == title then
      return action
    end
  end
  error("missing code action: " .. title)
end

local function stop_client(client_id)
  vim.lsp.stop_client(client_id, true)
  vim.wait(5000, function()
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
  local config_path = workspace .. "/fluent-lsp.toml"

  vim.fn.writefile({
    'origin_language = "en"',
    'file_masks = ["locales/{lang}/{filepath}.ftl"]',
  }, config_path)

  vim.cmd.edit(workspace .. "/locales/es/app.ftl")
  local client_id = start_client(server, workspace)
  wait_for_client(client_id, "codeActionProvider")

  local found = vim.fn.searchpos("coins-line", "n")
  vim.api.nvim_win_set_cursor(0, { found[1], found[2] - 1 })
  local actions = request_code_actions(client_id)
  assert(#actions == 3, "expected three style actions on message key")
  local action = find_action(actions, "Generate number selector (prefix)")
  assert(action.title == "Generate number selector (prefix)", "unexpected default action title: " .. vim.inspect(action))
  local edits = action.edit.documentChanges[1].edits
  assert(edits[1].snippet == "Tienes { \\$${1:coins} } { \\$${1:coins} ->\n    [one] monedas.\n    *[other] monedas.\n}", "unexpected default generated text")

  found = vim.fn.searchpos("\\$coins", "n")
  vim.api.nvim_win_set_cursor(0, { found[1], found[2] - 1 })
  actions = request_code_actions(client_id)
  assert(#actions == 3, "expected three style actions on variable")
  assert(find_action(actions, "Generate number selector from $coins (prefix)"), "missing prefix action")
  assert(find_action(actions, "Generate number selector from $coins (whole)"), "missing whole action")
  assert(find_action(actions, "Generate number selector from $coins (suffix)"), "missing suffix action")

  found = vim.fn.searchpos("plain-count", "n")
  vim.api.nvim_win_set_cursor(0, { found[1], found[2] - 1 })
  actions = request_code_actions(client_id)
  assert(#actions == 1, "plain message without variable should only offer whole")
  action = actions[1]
  edits = action.edit.documentChanges[1].edits
  assert(edits[1].snippet == "{ \\$${1:count} ->\n    [one] Monedas disponibles.\n    *[other] Monedas disponibles.\n}", "unexpected plain whole snippet")

  found = vim.fn.searchpos("range-summary", "n")
  vim.api.nvim_win_set_cursor(0, { found[1], found[2] - 1 })
  actions = request_code_actions_allow_empty(client_id)
  assert(#actions == 0, "ambiguous multi-variable message should not advertise actions")

  found = vim.fn.searchpos("nested-coins", "n")
  vim.api.nvim_win_set_cursor(0, { found[1], found[2] - 1 })
  actions = request_code_actions_allow_empty(client_id)
  assert(#actions == 0, "nested root message without a clear anchor should not advertise actions")

  found = vim.fn.searchpos("\\$files", "n")
  vim.api.nvim_win_set_cursor(0, { found[1], found[2] - 1 })
  actions = request_code_actions(client_id)
  assert(find_action(actions, "Generate number selector from $files (prefix)"), "missing attribute action")

  found = vim.fn.searchpos("formatted-download", "n")
  vim.api.nvim_win_set_cursor(0, { found[1], found[2] - 1 })
  actions = request_code_actions(client_id)
  assert(#actions == 3, "function-argument anchor should offer prefix, whole, and suffix generation")
  action = find_action(actions, "Generate number selector (prefix)")
  edits = action.edit.documentChanges[1].edits
  assert(edits[1].snippet == "Descarga { NUMBER(\\$${1:downloads}) } { \\$${1:downloads} ->\n    [one] archivos.\n    *[other] archivos.\n}", "unexpected function-argument generated text")

  found = vim.fn.searchpos("\\$downloads", "n")
  vim.api.nvim_win_set_cursor(0, { found[1], found[2] - 1 })
  actions = request_code_actions(client_id)
  assert(#actions == 3, "nested function variable under cursor should offer prefix, whole, and suffix generation")
  assert(find_action(actions, "Generate number selector from $downloads (prefix)"), "missing nested function variable prefix action")

  found = vim.fn.searchpos("formatted-download", "n")
  vim.api.nvim_win_set_cursor(0, { found[1], found[2] - 1 })
  found = vim.fn.searchpos("NUMBER($downloads)", "nW")
  vim.api.nvim_win_set_cursor(0, { found[1], found[2] - 1 })
  actions = request_code_actions(client_id)
  action = find_action(actions, "Generate number selector from NUMBER($downloads) (prefix)")
  edits = action.edit.documentChanges[1].edits
  assert(edits[1].snippet == "Descarga { NUMBER($downloads) } { NUMBER($downloads) ->\n    [one] archivos.\n    *[other] archivos.\n}", "unexpected function-call selector text")

  found = vim.fn.searchpos("deep-download", "n")
  vim.api.nvim_win_set_cursor(0, { found[1], found[2] - 1 })
  found = vim.fn.searchpos("WRAP(NUMBER($downloads))", "nW")
  vim.api.nvim_win_set_cursor(0, { found[1], found[2] - 1 })
  actions = request_code_actions(client_id)
  action = find_action(actions, "Generate number selector from WRAP(NUMBER($downloads)) (prefix)")
  edits = action.edit.documentChanges[1].edits
  assert(edits[1].snippet == "Descarga { WRAP(NUMBER($downloads)) } { WRAP(NUMBER($downloads)) ->\n    [one] archivos.\n    *[other] archivos.\n}", "unexpected nested function-call selector text")

  found = vim.fn.searchpos("coins-period", "n")
  vim.api.nvim_win_set_cursor(0, { found[1], found[2] - 1 })
  actions = request_code_actions(client_id)
  action = find_action(actions, "Generate number selector (prefix)")
  edits = action.edit.documentChanges[1].edits
  assert(edits[1].snippet == "Tienes { \\$${1:coins} } { \\$${1:coins} ->\n    [one].\n    *[other].\n}", "unexpected punctuation-preserving generated text")

  found = vim.fn.searchpos("whole-coins", "n")
  vim.api.nvim_win_set_cursor(0, { found[1], found[2] - 1 })
  actions = request_code_actions(client_id)
  action = find_action(actions, "Convert selector to prefix form")
  local change_uri = vim.uri_from_bufnr(0)
  assert(action.edit.changes[change_uri][1].newText == "Tienes { $coins } { $coins ->\n    [one] moneda.\n    *[other] monedas.\n}", "unexpected whole->prefix rewrite")
  action = find_action(actions, "Convert selector to suffix form")
  assert(action.edit.changes[change_uri][1].newText == "Tienes { $coins ->\n    [one] { $coins } moneda.\n    *[other] { $coins } monedas.\n}", "unexpected whole->suffix rewrite")

  found = vim.fn.searchpos("prefix-coins", "n")
  vim.api.nvim_win_set_cursor(0, { found[1], found[2] - 1 })
  actions = request_code_actions(client_id)
  action = find_action(actions, "Convert selector to whole form")
  assert(action.edit.changes[change_uri][1].newText == "{ $coins ->\n    [one] Tienes { $coins } moneda.\n    *[other] Tienes { $coins } monedas.\n}", "unexpected prefix->whole rewrite")

  found = vim.fn.searchpos("suffix-coins", "n")
  vim.api.nvim_win_set_cursor(0, { found[1], found[2] - 1 })
  actions = request_code_actions(client_id)
  assert(find_action(actions, "Convert selector to whole form"), "missing suffix->whole rewrite")
  assert(find_action(actions, "Convert selector to prefix form"), "missing suffix->prefix rewrite")

  found = vim.fn.searchpos("bare-suffix-coins", "n")
  vim.api.nvim_win_set_cursor(0, { found[1], found[2] - 1 })
  actions = request_code_actions(client_id)
  assert(#actions == 1, "bare suffix-like selector should only offer prefix collapse")
  action = find_action(actions, "Convert selector to prefix form")
  assert(action.edit.changes[change_uri][1].newText == "{ $coins } { $coins ->\n    [one] moneda.\n    *[other] monedas.\n}", "unexpected bare suffix-like prefix rewrite")

  found = vim.fn.searchpos("nested-whole-coins", "n")
  vim.api.nvim_win_set_cursor(0, { found[1], found[2] - 1 })
  found = vim.fn.searchpos("Ella tiene", "nW")
  vim.api.nvim_win_set_cursor(0, { found[1], found[2] - 1 })
  actions = request_code_actions(client_id)
  action = find_action(actions, "Convert selector to prefix form")
  assert(action.edit.changes[change_uri][1].newText == "Ella tiene { $coins } { $coins ->\n            [one] moneda.\n            *[other] monedas.\n        }", "unexpected nested whole->prefix rewrite")
  assert(find_action(actions, "Convert selector to suffix form"), "missing nested whole->suffix rewrite")

  found = vim.fn.searchpos("nested-coins", "n")
  vim.api.nvim_win_set_cursor(0, { found[1], found[2] - 1 })
  found = vim.fn.searchpos("\\$coins", "nW")
  vim.api.nvim_win_set_cursor(0, { found[1], found[2] - 1 })
  actions = request_code_actions(client_id)
  action = find_action(actions, "Generate number selector from $coins (prefix)")
  edits = action.edit.documentChanges[1].edits
  assert(edits[1].snippet == "Ella tiene { $coins } { $coins ->\n            [one] monedas.\n            *[other] monedas.\n        }", "unexpected nested generated text")

  stop_client(client_id)
  vim.fn.writefile({
    'origin_language = "en"',
    'file_masks = ["locales/{lang}/{filepath}.ftl"]',
    'selector_style = "whole"',
  }, config_path)

  client_id = start_client(server, workspace)
  wait_for_client(client_id, "codeActionProvider")
  local client = vim.lsp.get_client_by_id(client_id)
  assert(client, "missing restarted client")
  client.notify("workspace/didChangeConfiguration", {
    settings = {
      ["fluent-lsp"] = {
        selector_style = "prefix",
      },
    },
  })

  found = vim.fn.searchpos("coins-line", "n")
  vim.api.nvim_win_set_cursor(0, { found[1], found[2] - 1 })
  actions = request_code_actions(client_id)
  action = actions[1]
  assert(action.title == "Generate number selector (whole)", "file config should override client settings")
  edits = action.edit.documentChanges[1].edits
  assert(edits[1].snippet == "{ \\$${1:coins} ->\n    [one] Tienes { \\$${1:coins} } monedas.\n    *[other] Tienes { \\$${1:coins} } monedas.\n}", "unexpected whole generated text")

  write_result(result_path, {
    ok = true,
    feature = "code_action_generate_selector",
  })
  vim.cmd.qa()
end

return M
