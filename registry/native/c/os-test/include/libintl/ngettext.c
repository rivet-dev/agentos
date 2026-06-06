#include <libintl.h>
#ifdef ngettext
#undef ngettext
#endif
char *(*foo)(const char *, const char *, unsigned long int) = ngettext;
int main(void) { return 0; }
