#include <libintl.h>
#ifdef dcgettext_l
#undef dcgettext_l
#endif
char *(*foo)(const char *, const char *, int, locale_t) = dcgettext_l;
int main(void) { return 0; }
