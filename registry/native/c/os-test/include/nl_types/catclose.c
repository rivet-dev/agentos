#include <nl_types.h>
#ifdef catclose
#undef catclose
#endif
int (*foo)(nl_catd) = catclose;
int main(void) { return 0; }
