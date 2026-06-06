#include <libintl.h>
#ifdef gettext_l
#undef gettext_l
#endif
char *(*foo)(const char *, locale_t) = gettext_l;
int main(void) { return 0; }
