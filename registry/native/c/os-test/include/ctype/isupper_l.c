#include <ctype.h>
#ifdef isupper_l
#undef isupper_l
#endif
int (*foo)(int, locale_t) = isupper_l;
int main(void) { return 0; }
