#include <libintl.h>
#ifdef dcngettext_l
#undef dcngettext_l
#endif
char *(*foo)(const char *, const char *, const char *, unsigned long int, int, locale_t) = dcngettext_l;
int main(void) { return 0; }
