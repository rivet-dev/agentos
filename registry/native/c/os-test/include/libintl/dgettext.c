#include <libintl.h>
#ifdef dgettext
#undef dgettext
#endif
char *(*foo)(const char *, const char *) = dgettext;
int main(void) { return 0; }
