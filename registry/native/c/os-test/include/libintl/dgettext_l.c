#include <libintl.h>
#ifdef dgettext_l
#undef dgettext_l
#endif
char *(*foo)(const char *, const char *, locale_t) = dgettext_l;
int main(void) { return 0; }
