"""Management command to reindex all SearchableModel subclasses."""

from django.core.management.base import BaseCommand
from prismsearch_django.mixins import SearchableModel


class Command(BaseCommand):
    help = "Reindex all Prism-searchable models"

    def add_arguments(self, parser):
        parser.add_argument(
            "--model",
            type=str,
            help="Only reindex a specific model (app_label.ModelName)",
        )
        parser.add_argument(
            "--batch-size",
            type=int,
            default=500,
            help="Batch size for indexing (default: 500)",
        )

    def handle(self, *args, **options):
        batch_size = options["batch_size"]
        target_model = options.get("model")

        models = self._get_searchable_models()

        if target_model:
            models = [m for m in models if f"{m._meta.app_label}.{m.__name__}" == target_model]
            if not models:
                self.stderr.write(self.style.ERROR(f"Model {target_model} not found or not searchable"))
                return

        for model in models:
            label = f"{model._meta.app_label}.{model.__name__}"
            self.stdout.write(f"Reindexing {label} -> {model.PrismMeta.collection}...")
            try:
                count = model.prism.reindex(batch_size=batch_size)
                self.stdout.write(self.style.SUCCESS(f"  Indexed {count} documents"))
            except Exception as e:
                self.stderr.write(self.style.ERROR(f"  Error: {e}"))

    def _get_searchable_models(self):
        """Find all Django models that use SearchableModel."""
        from django.apps import apps

        result = []
        for model in apps.get_models():
            if issubclass(model, SearchableModel) and hasattr(model, "PrismMeta"):
                result.append(model)
        return result
