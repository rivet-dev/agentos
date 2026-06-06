#include <stdlib.h>
#ifdef div
#undef div
#endif
div_t (*foo)(int, int) = div;
int main(void) { return 0; }
