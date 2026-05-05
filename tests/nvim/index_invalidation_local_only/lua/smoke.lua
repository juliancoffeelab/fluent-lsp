local M = {}

local function write_result(path, payload)
  vim.fn.writefile({ vim.json.encode(payload) }, path)
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

local function attach_current(client_id)
  vim.lsp.buf_attach_client(0, client_id)
end

local function wait_client()
  local attached = vim.wait(3000, function()
    for _, client in ipairs(vim.lsp.get_clients({ bufnr = 0 })) do
      if client.name == "fluent-lsp" and client.server_capabilities.definitionProvider then
        return true
      end
    end
    return false
  end, 50)
  assert(attached, "fluent-lsp did not attach")
end

local function request(method, params)
  local result = vim.lsp.buf_request_sync(0, method, params, 3000)
  assert(result, method .. " timed out")
  for _, response in pairs(result) do
    assert(not response.err, method .. " failed: " .. vim.inspect(response.err))
    return response.result
  end
  error(method .. " returned no client response")
end

local function position_of(needle)
  local found = vim.fn.searchpos(needle, "n")
  assert(found[1] > 0, "missing " .. needle)
  return { line = found[1] - 1, character = found[2] - 1 }
end

local function set_lines(text)
  vim.api.nvim_buf_set_lines(0, 0, -1, false, vim.split(text, "\n", { plain = true }))
end

local function references_for_download()
  return request("textDocument/references", {
    textDocument = { uri = vim.uri_from_bufnr(0) },
    position = position_of("download-action"),
    context = { includeDeclaration = false },
  })
end

function M.run()
  local workspace = assert(vim.env.FLUENT_LSP_WORKSPACE ~= "" and vim.env.FLUENT_LSP_WORKSPACE)
  local server = assert(vim.env.FLUENT_LSP_BIN ~= "" and vim.env.FLUENT_LSP_BIN)
  local result_path = assert(vim.env.FLUENT_LSP_RESULT ~= "" and vim.env.FLUENT_LSP_RESULT)

  local es_app = workspace .. "/locales/es/app.ftl"
  local en_app = workspace .. "/locales/en/app.ftl"
  local fr_app = workspace .. "/locales/fr/app.ftl"
  local es_local = workspace .. "/locales/es/local.ftl"
  local en_local = workspace .. "/locales/en/local.ftl"
  local es_orphans = workspace .. "/locales/es/orphans.ftl"
  local en_orphans = workspace .. "/locales/en/orphans.ftl"

  vim.fn.writefile({ "hello = Hello", "", "# Edited docs", "fresh-key = Fresh value", "", "download-action = Download" }, en_app)
  vim.fn.writefile({ "hello = Hola" }, es_app)
  vim.fn.writefile({ "local-only = Solo local" }, es_local)
  vim.fn.writefile({ "shared = Hello", "menu =", "    .label = Save" }, en_orphans)
  vim.fn.writefile({
    "shared = Hola",
    "extra = Solo local",
    "menu =",
    "    .label = Guardar",
    "    .tooltip = Solo aqui",
  }, es_orphans)
  vim.fn.delete(en_local)
  vim.fn.delete(fr_app)

  vim.cmd.edit(es_app)
  local client_id = start_client(server, workspace)
  wait_client()

  vim.cmd.edit(en_app)
  attach_current(client_id)
  wait_client()
  set_lines("hello = Hello\n\n# Edited docs\nfresh-key = Fresh value\n\ndownload-action = Download")
  vim.cmd.write()

  vim.cmd.edit(es_app)
  attach_current(client_id)
  wait_client()
  set_lines("hello = Hola\n\nfresh-key = Fresco")
  local def = request("textDocument/definition", {
    textDocument = { uri = vim.uri_from_bufnr(0) },
    position = position_of("fresh-key"),
  })
  assert(def.uri:match("locales/en/app%.ftl$"), "definition did not use indexed origin")
  assert(def.range.start.line == 3, "definition line did not update after origin edit")

  local hover = request("textDocument/hover", {
    textDocument = { uri = vim.uri_from_bufnr(0) },
    position = position_of("fresh-key"),
  })
  assert(hover.contents.value:find("# Edited docs", 1, true), "hover missed edited origin docs")

  set_lines("fresh")
  local completion = request("textDocument/completion", {
    textDocument = { uri = vim.uri_from_bufnr(0) },
    position = { line = 0, character = 5 },
  })
  local labels = vim.tbl_map(function(item)
    return item.label
  end, completion.items or completion)
  assert(vim.tbl_contains(labels, "fresh-key"), "completion missed edited origin key")

  set_lines("hello = Hola\n\ndownload-action = Descargar")
  vim.wait(300, function()
    return false
  end, 300)
  vim.cmd.edit(en_app)
  attach_current(client_id)
  wait_client()
  local refs = references_for_download()
  assert(#refs == 1 and refs[1].uri:match("locales/es/app%.ftl$"), "references missed dirty translation")

  vim.cmd.edit(es_app)
  attach_current(client_id)
  wait_client()
  vim.cmd.bdelete({ bang = true })
  vim.cmd.edit(en_app)
  attach_current(client_id)
  wait_client()
  refs = references_for_download()
  assert(#refs == 0, "dirty close did not revert translation index")

  vim.fn.mkdir(workspace .. "/locales/fr", "p")
  vim.fn.writefile({ "download-action = Telecharger" }, fr_app)
  local added = vim.wait(3000, function()
    refs = references_for_download()
    return #refs == 1 and refs[1].uri:match("locales/fr/app%.ftl$")
  end, 100)
  assert(added, "background refresh missed added translation file")
  vim.uv.fs_unlink(fr_app)
  local removed = vim.wait(3000, function()
    refs = references_for_download()
    return #refs == 0
  end, 100)
  assert(removed, "background refresh kept deleted translation file")

  vim.cmd.edit(es_local)
  attach_current(client_id)
  wait_client()
  vim.cmd.write()
  local warned = vim.wait(3000, function()
    for _, diagnostic in ipairs(vim.diagnostic.get(0)) do
      if diagnostic.message:find("no origin%-language counterpart") then
        return true
      end
    end
    return false
  end, 50)
  assert(warned, "missing local-only warning diagnostic")

  vim.fn.mkdir(workspace .. "/locales/en", "p")
  vim.fn.writefile({ "local-only = Origin now exists" }, en_local)
  local cleared = vim.wait(3000, function()
    for _, diagnostic in ipairs(vim.diagnostic.get(0)) do
      if diagnostic.message:find("no origin%-language counterpart") then
        return false
      end
    end
    return true
  end, 100)
  assert(cleared, "background refresh did not clear local-only warning")

  vim.cmd.edit(es_orphans)
  attach_current(client_id)
  wait_client()
  vim.cmd.write()
  local warned_entries = vim.wait(3000, function()
    local saw_entry = false
    local saw_attribute = false
    for _, diagnostic in ipairs(vim.diagnostic.get(0)) do
      if diagnostic.message == "Translation entry `extra` has no origin-language counterpart" then
        saw_entry = true
      elseif diagnostic.message == "Translation attribute `menu.tooltip` has no origin-language counterpart" then
        saw_attribute = true
      end
    end
    return saw_entry and saw_attribute
  end, 50)
  assert(warned_entries, "missing translation-only entry warnings")

  vim.fn.writefile({
    "shared = Hello",
    "extra = Origin now exists",
    "menu =",
    "    .label = Save",
    "    .tooltip = Origin tooltip",
  }, en_orphans)
  local cleared_entries = vim.wait(3000, function()
    for _, diagnostic in ipairs(vim.diagnostic.get(0)) do
      if diagnostic.message == "Translation entry `extra` has no origin-language counterpart" then
        return false
      end
      if diagnostic.message == "Translation attribute `menu.tooltip` has no origin-language counterpart" then
        return false
      end
    end
    return true
  end, 100)
  assert(cleared_entries, "background refresh did not clear translation-only entry warnings")

  vim.fn.delete(en_local)
  vim.fn.delete(fr_app)

  write_result(result_path, {
    ok = true,
    feature = "index_invalidation_local_only",
  })
  vim.cmd.qa()
end

return M
