"use client";

import { useCallback, useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import { FileText, Loader2, Plus, RotateCcw, Trash2 } from 'lucide-react';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Textarea } from '@/components/ui/textarea';

interface TemplateInfo {
  id: string;
  name: string;
  description: string;
}

interface TemplateContent {
  id: string;
  content: string;
  source: 'custom' | 'bundled' | 'builtin';
  has_default: boolean;
}

interface TemplateEditorDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** Called after any save/delete so the parent can refresh its template list. */
  onTemplatesChanged: () => void;
}

const NEW_TEMPLATE_SKELETON = `{
  "name": "My Template",
  "description": "Describe when to use this template.",
  "sections": [
    {
      "title": "Summary",
      "instruction": "Provide a brief, one-paragraph executive summary of the entire meeting.",
      "format": "paragraph"
    },
    {
      "title": "Action Items",
      "instruction": "List all assigned tasks with their owners and due dates.",
      "format": "list"
    }
  ]
}
`;

/** Turn a display name into a filesystem-safe template id. */
function slugify(name: string): string {
  return name
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '_')
    .replace(/^_+|_+$/g, '');
}

/**
 * Full CRUD editor for summary templates.
 *
 * Templates are the prompts that drive summary generation. Defaults ship with
 * the app (bundled/built-in); saving one writes a custom override to the user
 * data directory, and deleting the override restores the default. Entirely
 * custom templates can be created and deleted freely.
 */
export function TemplateEditorDialog({
  open,
  onOpenChange,
  onTemplatesChanged,
}: TemplateEditorDialogProps) {
  const [templates, setTemplates] = useState<TemplateInfo[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [meta, setMeta] = useState<TemplateContent | null>(null);
  const [content, setContent] = useState('');
  const [dirty, setDirty] = useState(false);
  const [isNew, setIsNew] = useState(false);
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [saveAsOpen, setSaveAsOpen] = useState(false);
  const [saveAsName, setSaveAsName] = useState('');
  const [confirmingDelete, setConfirmingDelete] = useState(false);

  const refreshList = useCallback(async (): Promise<TemplateInfo[]> => {
    try {
      const list = await invoke<TemplateInfo[]>('api_list_templates');
      setTemplates(list);
      return list;
    } catch (err) {
      console.error('Failed to list templates:', err);
      toast.error('Failed to load templates', { description: String(err) });
      return [];
    }
  }, []);

  const loadTemplate = useCallback(async (id: string) => {
    setLoading(true);
    try {
      const data = await invoke<TemplateContent>('api_get_template_content', {
        templateId: id,
      });
      setSelectedId(id);
      setMeta(data);
      setContent(data.content);
      setDirty(false);
      setIsNew(false);
      setSaveAsOpen(false);
      setConfirmingDelete(false);
    } catch (err) {
      console.error('Failed to load template:', err);
      toast.error('Failed to load template', { description: String(err) });
    } finally {
      setLoading(false);
    }
  }, []);

  // Load the list (and first template) whenever the dialog opens.
  useEffect(() => {
    if (!open) return;
    void (async () => {
      const list = await refreshList();
      if (list.length > 0) await loadTemplate(list[0].id);
    })();
  }, [open, refreshList, loadTemplate]);

  const guardDirty = (): boolean => {
    if (!dirty) return true;
    return window.confirm('Discard unsaved changes to this template?');
  };

  const startNewTemplate = () => {
    if (!guardDirty()) return;
    setSelectedId(null);
    setMeta(null);
    setContent(NEW_TEMPLATE_SKELETON);
    setDirty(true);
    setIsNew(true);
    setSaveAsOpen(false);
    setSaveAsName('');
    setConfirmingDelete(false);
  };

  /** Parse the editor content, applying `name` when saving under a new name. */
  const contentWithName = (name?: string): string => {
    if (!name) return content;
    const parsed = JSON.parse(content);
    parsed.name = name;
    return JSON.stringify(parsed, null, 2);
  };

  const saveTo = async (id: string, json: string) => {
    setSaving(true);
    try {
      const name = await invoke<string>('api_save_template', {
        templateId: id,
        templateJson: json,
      });
      toast.success('Template saved', { description: `"${name}" saved as ${id}.json` });
      await refreshList();
      await loadTemplate(id);
      onTemplatesChanged();
    } catch (err) {
      toast.error('Failed to save template', { description: String(err) });
    } finally {
      setSaving(false);
    }
  };

  const handleSave = async () => {
    if (isNew || !selectedId) {
      // A new template needs a name/id first — reuse the Save As flow.
      setSaveAsOpen(true);
      return;
    }
    await saveTo(selectedId, content);
  };

  const handleSaveAs = async () => {
    const name = saveAsName.trim();
    if (!name) return;
    const id = slugify(name);
    if (!id) {
      toast.error('Invalid template name', {
        description: 'The name must contain at least one letter or digit.',
      });
      return;
    }
    if (templates.some((t) => t.id === id)) {
      toast.error('A template with this name already exists', {
        description: `"${name}" would overwrite ${id}.json. Pick a different name.`,
      });
      return;
    }
    let json: string;
    try {
      json = contentWithName(name);
    } catch {
      toast.error('Invalid JSON', { description: 'Fix the JSON before saving.' });
      return;
    }
    setSaveAsOpen(false);
    setSaveAsName('');
    await saveTo(id, json);
  };

  const handleDelete = async () => {
    if (!selectedId || !meta) return;
    if (!confirmingDelete) {
      setConfirmingDelete(true);
      return;
    }
    setConfirmingDelete(false);
    try {
      const reverted = await invoke<boolean>('api_delete_template', {
        templateId: selectedId,
      });
      toast.success(reverted ? 'Template reset to default' : 'Template deleted');
      const list = await refreshList();
      onTemplatesChanged();
      if (reverted) {
        await loadTemplate(selectedId);
      } else if (list.length > 0) {
        await loadTemplate(list[0].id);
      } else {
        startNewTemplate();
      }
    } catch (err) {
      toast.error('Failed to delete template', { description: String(err) });
    }
  };

  // Live JSON syntax feedback (structure is validated by the backend on save).
  let jsonError: string | null = null;
  try {
    JSON.parse(content);
  } catch (err) {
    jsonError = err instanceof Error ? err.message : 'Invalid JSON';
  }

  const isCustom = meta?.source === 'custom';
  const deleteLabel = isCustom && meta?.has_default ? 'Reset to default' : 'Delete';
  const DeleteIcon = isCustom && meta?.has_default ? RotateCcw : Trash2;

  const sourceBadge = isNew
    ? { label: 'New', className: 'bg-blue-50 text-blue-700 border-blue-200' }
    : isCustom
      ? meta?.has_default
        ? { label: 'Default (edited)', className: 'bg-amber-50 text-amber-700 border-amber-200' }
        : { label: 'Custom', className: 'bg-green-50 text-green-700 border-green-200' }
      : { label: 'Default', className: 'bg-gray-50 text-gray-600 border-gray-200' };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-4xl h-[80vh] flex flex-col gap-3 p-5">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <FileText size={18} />
            Summary Templates
          </DialogTitle>
          <DialogDescription>
            Templates define the sections and instructions used to generate summaries. Edit the
            JSON, save it as-is or under a new name. Edited defaults can be reset at any time.
          </DialogDescription>
        </DialogHeader>

        <div className="flex flex-1 min-h-0 gap-4">
          {/* Template list */}
          <div className="w-56 flex flex-col border border-gray-200 rounded-md overflow-hidden">
            <div className="flex-1 overflow-y-auto py-1">
              {templates.map((t) => (
                <button
                  key={t.id}
                  type="button"
                  title={t.description}
                  onClick={() => {
                    if (t.id === selectedId && !isNew) return;
                    if (!guardDirty()) return;
                    void loadTemplate(t.id);
                  }}
                  className={`w-full text-left px-3 py-2 text-sm hover:bg-gray-50 ${
                    t.id === selectedId && !isNew
                      ? 'bg-blue-50 text-blue-700 font-medium'
                      : 'text-gray-800'
                  }`}
                >
                  <div className="truncate">{t.name}</div>
                  <div className="text-xs text-gray-400 truncate">{t.id}</div>
                </button>
              ))}
            </div>
            <button
              type="button"
              onClick={startNewTemplate}
              className="flex items-center gap-1.5 px-3 py-2 text-sm text-gray-600 hover:bg-gray-50 border-t border-gray-200"
            >
              <Plus size={14} />
              New template
            </button>
          </div>

          {/* Editor */}
          <div className="flex-1 flex flex-col min-w-0 gap-2">
            <div className="flex items-center gap-2">
              <span
                className={`inline-flex items-center rounded-full border px-2 py-0.5 text-xs font-medium ${sourceBadge.className}`}
              >
                {sourceBadge.label}
              </span>
              {selectedId && !isNew && (
                <span className="text-xs text-gray-400 truncate">{selectedId}.json</span>
              )}
              {dirty && <span className="text-xs text-amber-600">Unsaved changes</span>}
            </div>

            <Textarea
              value={content}
              onChange={(e) => {
                setContent(e.target.value);
                setDirty(true);
              }}
              disabled={loading}
              spellCheck={false}
              className="flex-1 min-h-0 resize-none font-mono text-xs leading-5"
            />

            {jsonError && (
              <p className="text-xs text-red-600 truncate" title={jsonError}>
                Invalid JSON: {jsonError}
              </p>
            )}

            {saveAsOpen ? (
              <div className="flex items-center gap-2">
                <Input
                  autoFocus
                  value={saveAsName}
                  onChange={(e) => setSaveAsName(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === 'Enter') void handleSaveAs();
                    if (e.key === 'Escape') setSaveAsOpen(false);
                  }}
                  placeholder="New template name..."
                  className="h-8 text-sm"
                />
                <Button
                  size="sm"
                  onClick={() => void handleSaveAs()}
                  disabled={saving || !saveAsName.trim() || !!jsonError}
                >
                  {saving ? <Loader2 className="animate-spin" size={14} /> : 'Save'}
                </Button>
                <Button size="sm" variant="outline" onClick={() => setSaveAsOpen(false)}>
                  Cancel
                </Button>
              </div>
            ) : (
              <div className="flex items-center justify-between">
                <div className="flex items-center gap-2">
                  <Button
                    size="sm"
                    onClick={() => void handleSave()}
                    disabled={saving || loading || !!jsonError || (!dirty && !isNew)}
                  >
                    {saving ? <Loader2 className="animate-spin" size={14} /> : 'Save'}
                  </Button>
                  {!isNew && selectedId && (
                    <Button
                      size="sm"
                      variant="outline"
                      onClick={() => {
                        setSaveAsName('');
                        setSaveAsOpen(true);
                      }}
                      disabled={saving || loading || !!jsonError}
                    >
                      Save as new...
                    </Button>
                  )}
                </div>

                {isCustom && selectedId && (
                  <Button
                    size="sm"
                    variant="outline"
                    onClick={() => void handleDelete()}
                    onBlur={() => setConfirmingDelete(false)}
                    className={
                      confirmingDelete
                        ? 'border-red-300 bg-red-50 text-red-700 hover:bg-red-100'
                        : 'text-gray-600'
                    }
                  >
                    <DeleteIcon size={14} className="mr-1.5" />
                    {confirmingDelete ? `Confirm ${deleteLabel.toLowerCase()}?` : deleteLabel}
                  </Button>
                )}
              </div>
            )}
          </div>
        </div>
      </DialogContent>
    </Dialog>
  );
}
