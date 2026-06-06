#include <libintl.h>
#ifdef ngettext_l
#undef ngettext_l
#endif
char *(*foo)(const char *, const char *, unsigned long int, locale_t) = ngettext_l;
int main(void) { return 0; }
