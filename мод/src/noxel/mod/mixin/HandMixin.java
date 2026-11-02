package noxel.mod.mixin;

import com.mojang.blaze3d.vertex.PoseStack;
import net.minecraft.client.player.AbstractClientPlayer;
import net.minecraft.client.renderer.ItemInHandRenderer;
import net.minecraft.client.renderer.SubmitNodeCollector;
import net.minecraft.world.InteractionHand;
import net.minecraft.world.item.ItemStack;
import noxel.mod.vis.ViewModel;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfo;

/** Рука от первого лица: положение, размер и свои анимации (удар, доставание, осмотр). Если вставка не сработает, игра просто идёт без них. */
@Mixin(ItemInHandRenderer.class)
public class HandMixin {
    @Inject(method = "submitArmWithItem", at = @At("HEAD"), require = 0)
    private void noxel$hand(AbstractClientPlayer player, float partial, float pitch, InteractionHand hand, float swing, ItemStack stack, float equip, PoseStack poseStack, SubmitNodeCollector collector, int light, CallbackInfo ci) {
        try {
            ViewModel.apply(player, hand, swing, equip, stack, poseStack);
        } catch (Throwable ignored) {
        }
    }
}
