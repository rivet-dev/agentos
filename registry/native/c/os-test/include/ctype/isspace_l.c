#include <ctype.h>
#ifdef isspace_l
#undef isspace_l
#endif
int (*foo)(int, locale_t) = isspace_l;
int main(void) { return 0; }
