#include <libintl.h>
#ifdef gettext
#undef gettext
#endif
char *(*foo)(const char *) = gettext;
int main(void) { return 0; }
