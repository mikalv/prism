"""Django app configuration for prismsearch_django."""

from django.apps import AppConfig


class PrismsearchConfig(AppConfig):
    name = "prismsearch_django"
    verbose_name = "Prismsearch"
    default_auto_field = "django.db.models.BigAutoField"

    def ready(self):
        from django.db.models.signals import post_save, post_delete
        from prismsearch_django.signals import post_save_handler, post_delete_handler

        post_save.connect(post_save_handler)
        post_delete.connect(post_delete_handler)
