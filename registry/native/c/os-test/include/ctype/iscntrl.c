#include <ctype.h>
#ifdef iscntrl
#undef iscntrl
#endif
int (*foo)(int) = iscntrl;
int main(void) { return 0; }
