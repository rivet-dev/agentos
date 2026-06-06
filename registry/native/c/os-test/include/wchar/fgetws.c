#include <wchar.h>
#ifdef fgetws
#undef fgetws
#endif
wchar_t *(*foo)(wchar_t *restrict, int, FILE *restrict) = fgetws;
int main(void) { return 0; }
