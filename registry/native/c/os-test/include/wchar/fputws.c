#include <wchar.h>
#ifdef fputws
#undef fputws
#endif
int (*foo)(const wchar_t *restrict, FILE *restrict) = fputws;
int main(void) { return 0; }
