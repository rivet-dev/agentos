#include <libintl.h>
#ifdef dngettext
#undef dngettext
#endif
char *(*foo)(const char *, const char *, const char *, unsigned long int) = dngettext;
int main(void) { return 0; }
