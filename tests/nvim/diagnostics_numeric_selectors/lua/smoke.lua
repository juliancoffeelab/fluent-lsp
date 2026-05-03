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

local function save_current_buffer(client)
  client.notify("textDocument/didSave", {
    textDocument = {
      uri = vim.uri_from_bufnr(0),
    },
    text = table.concat(vim.api.nvim_buf_get_lines(0, 0, -1, false), "\n"),
  })
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

  vim.cmd.edit(workspace .. "/locales/lv/app.ftl")
  local client_id = start_client(server, workspace)
  wait_for_client(client_id)

  local diagnostics = wait_for_diagnostics_count(0)
  assert(#diagnostics == 0, "diagnostics should be off by default")

  local client = assert(vim.lsp.get_client_by_id(client_id), "missing client")
  client.notify("workspace/didChangeConfiguration", {
    settings = {
      ["fluent-lsp"] = {
        error_on_unsupported_plural_categories = true,
        warn_on_missing_plural_categories = true,
      },
    },
  })
  save_current_buffer(client)

  diagnostics = wait_for_diagnostics_count(4)
  local messages = {}
  local zero_count = 0
  local one_count = 0
  local unsupported = nil
  for _, diagnostic in ipairs(diagnostics) do
    table.insert(messages, diagnostic.message)
    if diagnostic.message == "Numeric selector for `lv` is missing category `zero`" then
      zero_count = zero_count + 1
    elseif diagnostic.message == "Numeric selector for `lv` is missing category `one`" then
      one_count = one_count + 1
    elseif diagnostic.message == "`few` is not a supported plural category for `lv`" then
      unsupported = diagnostic
    end
  end
  assert(unsupported, "missing unsupported-category diagnostic")
  assert(unsupported.severity == vim.diagnostic.severity.ERROR, "unsupported category should be an error")
  assert(zero_count == 2, "expected two missing-zero warnings")
  assert(one_count == 1, "expected one missing-one warning")

  vim.cmd.edit(workspace .. "/locales/uk/app.ftl")
  vim.lsp.buf_attach_client(0, client_id)
  wait_for_client(client_id)
  client = assert(vim.lsp.get_client_by_id(client_id), "missing client after buffer switch")
  save_current_buffer(client)
  diagnostics = wait_for_diagnostics_count(1)
  assert(diagnostics[1].message == "Numeric selector for `uk` is missing category `one`", "unexpected Ukrainian diagnostics")
  assert(not diagnostics[1].message:find("few", 1, true), "Ukrainian `few` should be supported")
  assert(not diagnostics[1].message:find("many", 1, true), "Ukrainian `many` should be supported")

  vim.cmd.edit(workspace .. "/locales/en/app.ftl")
  vim.lsp.buf_attach_client(0, client_id)
  wait_for_client(client_id)
  client = assert(vim.lsp.get_client_by_id(client_id), "missing client after second buffer switch")
  client.notify("workspace/didChangeConfiguration", {
    settings = {
      ["fluent-lsp"] = {
        error_on_unsupported_plural_categories = true,
        warn_on_selector_style_mismatch = true,
        selector_style = "prefix",
      },
    },
  })
  save_current_buffer(client)

  diagnostics = wait_for_diagnostics_count(4)
  local style_messages = {}
  local whole_count = 0
  local suffix_count = 0
  local bad_key = nil
  for _, diagnostic in ipairs(diagnostics) do
    if diagnostic.message == "Selector style is `whole`, but workspace prefers `prefix`" then
      whole_count = whole_count + 1
      table.insert(style_messages, diagnostic.message)
      assert(diagnostic.severity == vim.diagnostic.severity.WARN, "style mismatch should be a warning")
    elseif diagnostic.message == "Selector style is `suffix`, but workspace prefers `prefix`" then
      suffix_count = suffix_count + 1
      table.insert(style_messages, diagnostic.message)
      assert(diagnostic.severity == vim.diagnostic.severity.WARN, "style mismatch should be a warning")
    elseif diagnostic.message == "`admins` is not a supported numeric selector key for `en`; use exact numbers or plural categories" then
      bad_key = diagnostic
    end
  end
  assert(whole_count == 2, "expected two whole-style warnings including local selector occurrence")
  assert(suffix_count == 1, "expected one suffix-style warning")
  assert(bad_key, "missing invalid numeric identifier diagnostic")
  assert(bad_key.severity == vim.diagnostic.severity.ERROR, "bad numeric key should be an error")

  vim.cmd.edit(workspace .. "/locales/en/match.ftl")
  vim.lsp.buf_attach_client(0, client_id)
  wait_for_client(client_id)
  client = assert(vim.lsp.get_client_by_id(client_id), "missing client after match buffer switch")
  client.notify("workspace/didChangeConfiguration", {
    settings = {
      ["fluent-lsp"] = {
        warn_on_missing_plural_categories = true,
        warn_on_selector_style_mismatch = true,
        selector_style = "whole",
      },
    },
  })
  save_current_buffer(client)
  diagnostics = wait_for_diagnostics_count(0)
  assert(#diagnostics == 0, "matching whole selector should stay quiet")

  local en_buf = vim.api.nvim_get_current_buf()
  vim.cmd.enew()
  vim.api.nvim_buf_delete(en_buf, { force = true })
  local cleared = vim.wait(3000, function()
    for _, diagnostic in ipairs(vim.diagnostic.get()) do
      if diagnostic.bufnr == en_buf then
        return false
      end
    end
    return true
  end, 50)
  assert(cleared, "expected diagnostics to clear after closing the buffer")

  stop_client(client_id)
  write_result(result_path, { ok = true })
  vim.cmd("qa!")
end

return M
