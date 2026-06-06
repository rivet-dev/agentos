#include <ctype.h>
#ifdef isalpha_l
#undef isalpha_l
#endif
int (*foo)(int, locale_t) = isalpha_l;
int main(void) { return 0; }
