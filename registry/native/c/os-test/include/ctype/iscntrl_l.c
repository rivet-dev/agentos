#include <ctype.h>
#ifdef iscntrl_l
#undef iscntrl_l
#endif
int (*foo)(int, locale_t) = iscntrl_l;
int main(void) { return 0; }
