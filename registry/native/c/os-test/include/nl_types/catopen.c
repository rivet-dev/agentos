#include <nl_types.h>
#ifdef catopen
#undef catopen
#endif
nl_catd (*foo)(const char *, int) = catopen;
int main(void) { return 0; }
