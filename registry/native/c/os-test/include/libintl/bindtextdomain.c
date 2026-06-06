#include <libintl.h>
#ifdef bindtextdomain
#undef bindtextdomain
#endif
char *(*foo)(const char *, const char *) = bindtextdomain;
int main(void) { return 0; }
