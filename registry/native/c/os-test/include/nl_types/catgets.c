#include <nl_types.h>
#ifdef catgets
#undef catgets
#endif
char *(*foo)(nl_catd, int, int, const char *) = catgets;
int main(void) { return 0; }
