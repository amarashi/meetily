import { useState, useEffect, useCallback } from 'react';
import { invoke as invokeTauri } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import Analytics from '@/lib/analytics';

export function useTemplates() {
  const [availableTemplates, setAvailableTemplates] = useState<Array<{
    id: string;
    name: string;
    description: string;
  }>>([]);
  const [selectedTemplate, setSelectedTemplate] = useState<string>('standard_meeting');

  const refreshTemplates = useCallback(async () => {
    try {
      const templates = await invokeTauri('api_list_templates') as Array<{
        id: string;
        name: string;
        description: string;
      }>;
      console.log('Available templates:', templates);
      setAvailableTemplates(templates);
      // If the selected template was deleted in the editor, fall back to the default.
      setSelectedTemplate((current) =>
        templates.some((t) => t.id === current) ? current : 'standard_meeting',
      );
    } catch (error) {
      console.error('Failed to fetch templates:', error);
    }
  }, []);

  // Fetch available templates on mount
  useEffect(() => {
    refreshTemplates();
  }, [refreshTemplates]);

  // Handle template selection
  const handleTemplateSelection = useCallback((templateId: string, templateName: string) => {
    setSelectedTemplate(templateId);
    toast.success('Template selected', {
      description: `Using "${templateName}" template for summary generation`,
    });
    Analytics.trackFeatureUsed('template_selected');
  }, []);

  return {
    availableTemplates,
    selectedTemplate,
    handleTemplateSelection,
    refreshTemplates,
  };
}
