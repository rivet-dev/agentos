#include <libintl.h>
#ifdef textdomain
#undef textdomain
#endif
char *(*foo)(const char *) = textdomain;
int main(void) { return 0; }
