#include <ctype.h>
#ifdef toupper_l
#undef toupper_l
#endif
int (*foo)(int, locale_t) = toupper_l;
int main(void) { return 0; }
