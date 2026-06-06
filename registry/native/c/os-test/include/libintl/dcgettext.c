#include <libintl.h>
#ifdef dcgettext
#undef dcgettext
#endif
char *(*foo)(const char *, const char *, int) = dcgettext;
int main(void) { return 0; }
