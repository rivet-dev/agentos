#include <libintl.h>
#ifdef dngettext_l
#undef dngettext_l
#endif
char *(*foo)(const char *, const char *, const char *, unsigned long int, locale_t) = dngettext_l;
int main(void) { return 0; }
